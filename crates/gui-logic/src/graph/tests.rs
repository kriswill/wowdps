use super::samples::{ITEM_KINDS, mark, marked, role_marked, timeline};
use super::*;
use crate::theme::{NAVY, themes};

/// The shared y-scale is the whole point: a player doing half the damage
/// must draw half as tall, not identically.
#[test]
fn peak_spans_both_sides() {
    let small = timeline(vec![100, 100]);
    let big = timeline(vec![1000, 1000]);
    let peak = peak_of(&[&small, &big], GraphMode::Total, (0, 2));
    assert_eq!(peak, 2000.0);
    assert!(
        peak > curve(&small, GraphMode::Total)
            .into_iter()
            .fold(0.0, f64::max)
    );
}

#[test]
fn cumulative_is_monotonic_and_dps_is_not() {
    let t = timeline(vec![10, 0, 0, 90]);
    let total = curve(&t, GraphMode::Total);
    assert!(total.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(total.last().copied(), Some(100.0));
    // The DPS curve dips through the dead air; the cumulative cannot.
    let dps = curve(&t, GraphMode::Dps);
    assert_eq!(dps.len(), 4);
}

#[test]
fn names_lose_their_realm() {
    assert_eq!(short_name("Keanucleavês-Proudmoore"), "Keanucleavês");
    assert_eq!(short_name("Alice"), "Alice");
}

#[test]
fn marker_colors_and_names_are_distinct_per_kind() {
    // R18: every kind has a name and a colour; the exhaustive list is
    // the model's ten, in code order. Names are all distinct; colours
    // too, except the one documented pair — active mitigation and
    // defensives share the coral.
    let kinds = ALL_KINDS;
    assert_eq!(kinds.len(), 10);
    for (i, k) in kinds.iter().enumerate() {
        assert_eq!(k.code() as usize, i, "{k:?} out of code order");
    }
    let shares_a_hue = |a: MarkKind, b: MarkKind| {
        matches!(
            (a, b),
            (MarkKind::ActiveMitigation, MarkKind::Defensive)
                | (MarkKind::Defensive, MarkKind::ActiveMitigation)
        )
    };
    for def in themes() {
        let color = |k| mark_color(k, &def.data);
        for (i, a) in kinds.iter().enumerate() {
            for b in kinds.iter().skip(i + 1) {
                if shares_a_hue(*a, *b) {
                    assert_eq!(color(*a), color(*b));
                } else {
                    assert_ne!(color(*a), color(*b), "{}: {a:?} vs {b:?}", def.name);
                }
                assert_ne!(mark_name(*a), mark_name(*b));
            }
        }
    }
    let color = |k| mark_color(k, &NAVY.data);
    assert_eq!(
        color(MarkKind::ActiveMitigation),
        Color::rgb(1.0, 0.45, 0.40)
    );
    assert_eq!(color(MarkKind::Cooldown), Color::rgb(0.50, 0.40, 1.0));
    assert_eq!(color(MarkKind::Consumable), NAVY.overlay.good);
    assert_eq!(mark_name(MarkKind::ActiveMitigation), "mitigation");
    assert_eq!(mark_name(MarkKind::Defensive), "defensive");
    assert_eq!(mark_name(MarkKind::SupportBuff), "support");
    assert_eq!(mark_name(MarkKind::Cooldown), "cooldown");
    assert_eq!(mark_name(MarkKind::Consumable), "consumable");
}

#[test]
fn the_view_window_clamps_and_falls_back() {
    assert_eq!(view_window(None, 1000, 10), (0, 10));
    assert_eq!(view_window(Some((1_000, 3_500)), 1000, 10), (1, 4));
    assert_eq!(
        view_window(Some((0, 99_000)), 1000, 10),
        (0, 10),
        "past the data"
    );
    assert_eq!(
        view_window(Some((5_000, 5_000)), 1000, 10),
        (0, 10),
        "zero width"
    );
    assert_eq!(view_window(Some((9_000, 20_000)), 1000, 10), (9, 10));
    assert_eq!(
        view_window(Some((20_000, 30_000)), 1000, 10),
        (0, 10),
        "beyond"
    );
    assert_eq!(mmss(0), "0:00");
    assert_eq!(mmss(83_000), "1:23");
    assert_eq!(mmss(3_599_999), "59:59");
}

#[test]
fn the_peak_is_taken_over_the_displayed_window_only() {
    let t = timeline(vec![10, 1000, 10, 10]);
    assert_eq!(peak_of(&[&t], GraphMode::Total, (0, 4)), 1030.0);
    assert_eq!(peak_of(&[&t], GraphMode::Total, (0, 1)), 10.0);
    assert_eq!(peak_of(&[&t], GraphMode::Total, (2, 4)), 1030.0);
    assert_eq!(
        peak_of(&[&t], GraphMode::Total, (9, 12)),
        0.0,
        "window past the data"
    );
    assert_eq!(peak_of(&[], GraphMode::Dps, (0, 4)), 0.0);
}

#[test]
fn hover_lines_count_uses_and_uptime_across_both_sides() {
    let a = marked();
    let mut b = marked();
    b.marks.retain(|m| m.label == "Trinket");
    let (kind, name, details) = hover_line(&[&a, &b], "Trinket", (0, 10), &[], true, &[]).unwrap();
    assert_eq!(kind, MarkKind::TrinketUse);
    assert_eq!(name, "Trinket");
    assert_eq!(details, "trinket use ×2 · uptime 20s · 100%");
    let (_, _, details) = hover_line(&[&a], "Proc", (0, 10), &[], true, &[]).unwrap();
    assert_eq!(details, "proc ×1", "no duration, no uptime clause");
    let (_, _, details) = hover_line(&[&a], "Trinket", (0, 40), &[], true, &[]).unwrap();
    assert_eq!(details, "trinket use ×1 · uptime 10s · 25%");
    assert!(hover_line(&[&a], "Nothing", (0, 10), &[], true, &[]).is_none());
}

/// v27: a comparison's total hides the answer the reader wants — WHO
/// popped it — so each side's own count follows it, zeroes included.
#[test]
fn a_hovered_marker_splits_its_count_by_side() {
    let a = marked();
    let mut b = marked();
    b.marks.retain(|m| m.label == "Trinket");
    let sides = ["Alice".to_string(), "Bob".to_string()];
    let (_, _, details) = hover_line(&[&a, &b], "Trinket", (0, 10), &[], true, &sides).unwrap();
    assert_eq!(
        details,
        "trinket use ×2 (Alice 1 · Bob 1) · uptime 20s · 100%"
    );
    // A side that never used it is still named: "Bob 0" IS the finding.
    let (_, _, details) = hover_line(&[&a, &b], "Proc", (0, 10), &[], true, &sides).unwrap();
    assert_eq!(details, "proc ×1 (Alice 1 · Bob 0)");
    // One side, no split — the drilldown's legend is unchanged.
    let (_, _, details) = hover_line(&[&a], "Trinket", (0, 10), &[], true, &sides[..1]).unwrap();
    assert_eq!(details, "trinket use ×1 · uptime 10s · 100%");
}

/// The probe is an INSTANT: the same bucket on every graph, read on each
/// curve. Two sides name themselves; one side stays bare.
#[test]
fn the_probe_reads_one_instant_on_every_curve() {
    let sides = vec![
        ("Alice".to_string(), vec![1.0, 2_000.0, 3.0]),
        ("Bob".to_string(), vec![4.0, 512_000.0, 6.0]),
    ];
    // One reading PER GRAPH, in graph order: the legend puts each under
    // its own curve, so the order and the count are load-bearing.
    let (when, values) = probe_line(1, 1000, "dps", &sides);
    assert_eq!(when, "0:01");
    assert_eq!(values, ["Alice 2.0k", "Bob 512.0k"]);
    // One side: no name, the drilldown's old wording with the metric on
    // the number, since there is no pair to tell apart.
    let (when, values) = probe_line(2, 5000, "hps", &sides[..1]);
    assert_eq!(when, "0:10");
    assert_eq!(values, ["hps: 3"]);
    // A side whose curve is shorter keeps its SLOT with a dash: dropping
    // it would slide the other reading across the divider and label the
    // wrong player's graph.
    let short = vec![
        ("Alice".to_string(), vec![1.0]),
        ("Bob".to_string(), vec![4.0, 5.0]),
    ];
    assert_eq!(probe_line(1, 1000, "dps", &short).1, ["Alice —", "Bob 5"]);
    // Past both curves: the instant still reads, the numbers do not.
    assert_eq!(
        probe_line(9, 1000, "dps", &short),
        (
            "0:09".to_string(),
            vec!["Alice —".to_string(), "Bob —".to_string()]
        )
    );
}

/// R18: the legend keys only the kinds with a mark inside the displayed
/// window — item marks alone still give the four R12 keys, a tank's
/// graph adds theirs, and a window with no marks keys nothing.
#[test]
fn the_legend_keys_only_the_kinds_shown() {
    assert_eq!(kinds_shown(&[&marked()], (0, 10)), ITEM_KINDS);
    let role = role_marked();
    assert_eq!(
        kinds_shown(&[&role], (0, 10)),
        [
            MarkKind::TrinketProc,
            MarkKind::External,
            MarkKind::ActiveMitigation,
            MarkKind::Defensive,
            MarkKind::SupportBuff,
            MarkKind::Cooldown,
            MarkKind::Death,
            MarkKind::HealingCooldown,
        ]
    );
    // Both graphs of a comparison pool their kinds, in code order.
    assert_eq!(kinds_shown(&[&marked(), &role], (0, 10)), ALL_KINDS);
    // A zoomed window keeps every kind with a span DRAWN in it — a span
    // that began earlier and runs into the window earns its key — and
    // drops the rest: the expectation is computed from the marks.
    let overlapping: Vec<MarkKind> = ALL_KINDS
        .into_iter()
        .filter(|k| {
            role.marks
                .iter()
                .any(|m| m.kind == *k && m.at_ms + m.dur_ms.max(0) >= 8_000 && m.at_ms <= 10_000)
        })
        .collect();
    assert_eq!(kinds_shown(&[&role], (8, 10)), overlapping);
    assert!(overlapping.contains(&MarkKind::ActiveMitigation));
    // A window past every span is empty.
    assert!(kinds_shown(&[&role], (30, 40)).is_empty());
    assert!(kinds_shown(&[&Timeline::default()], (0, 10)).is_empty());
}

/// R18: the hover names the caster — resolved to a name when the graph
/// knows the guid, else the guid's tail — and item marks stay as before.
#[test]
fn hover_lines_name_the_caster() {
    let t = role_marked();
    let names = [("Player-1-0B", "Gennar"), ("Player-1-0A", "Tank")];
    let (kind, name, details) =
        hover_line(&[&t], "Pain Suppression", (0, 10), &names, true, &[]).unwrap();
    assert_eq!(kind, MarkKind::External);
    assert_eq!(name, "Pain Suppression from Gennar");
    assert_eq!(details, "external ×1 · uptime 8s · 80%");
    // An unknown guid shows its tail rather than nothing.
    let (_, name, _) = hover_line(&[&t], "Ebon Might", (0, 10), &names, true, &[]).unwrap();
    assert_eq!(name, "Ebon Might from 0E");
    // Two marks of one label from one caster name them once.
    let (_, name, details) = hover_line(&[&t], "Shield Block", (0, 10), &names, true, &[]).unwrap();
    assert_eq!(name, "Shield Block from Tank");
    assert_eq!(details, "mitigation ×2 · uptime 8s · 80%");
    // Both sides of a comparison: the same external from two priests.
    let mut u = role_marked();
    u.marks.retain(|m| m.label == "Pain Suppression");
    u.marks[0].src = "Player-1-0C".to_string();
    let (_, name, _) =
        hover_line(&[&t, &u], "Pain Suppression", (0, 10), &names, true, &[]).unwrap();
    assert_eq!(name, "Pain Suppression from Gennar, 0C");
    // No caster, no clause.
    let (_, name, _) = hover_line(&[&t], "Proc", (0, 10), &names, true, &[]).unwrap();
    assert_eq!(name, "Proc");
    // The overlay (with_caster = false) keeps the bare label: its legend
    // row is too narrow for the clause, and a wrapped line misbehaves.
    let (kind, name, details) =
        hover_line(&[&t], "Pain Suppression", (0, 10), &names, false, &[]).unwrap();
    assert_eq!(kind, MarkKind::External);
    assert_eq!(name, "Pain Suppression");
    assert_eq!(details, "external ×1 · uptime 8s · 80%");
    assert_eq!(caster_name("Player-1-0A", &names), "Tank");
    assert_eq!(caster_name("Creature-0-1-2-3-4-5", &names), "5");
    assert_eq!(caster_name("nohyphen", &names), "nohyphen");
}

/// v34: each view draws the marks about its own metric — cooldowns and
/// support on the throughput curves, mitigation and defensives on the
/// taken curve, healing cooldowns on healing alone; items, externals and
/// deaths on every one — and the narrowed timeline keeps its buckets.
#[test]
fn each_view_draws_the_marks_about_its_metric() {
    let everywhere = [
        MarkKind::TrinketUse,
        MarkKind::TrinketProc,
        MarkKind::Consumable,
        MarkKind::External,
        MarkKind::Death,
    ];
    for view in [
        View::Damage,
        View::Healing,
        View::Taken,
        View::EnemyTaken,
        View::Deaths,
    ] {
        for k in everywhere {
            assert!(view_draws(view, k), "{view:?} {k:?}");
        }
    }
    let on = |view: View| -> Vec<MarkKind> {
        ALL_KINDS
            .into_iter()
            .filter(|k| view_draws(view, *k))
            .collect()
    };
    assert!(on(View::Damage).contains(&MarkKind::Cooldown));
    assert!(on(View::Damage).contains(&MarkKind::SupportBuff));
    assert!(!on(View::Damage).contains(&MarkKind::Defensive));
    assert!(!on(View::Damage).contains(&MarkKind::ActiveMitigation));
    assert!(!on(View::Damage).contains(&MarkKind::HealingCooldown));
    assert!(on(View::Healing).contains(&MarkKind::Cooldown));
    assert!(on(View::Healing).contains(&MarkKind::HealingCooldown));
    assert!(!on(View::Healing).contains(&MarkKind::Defensive));
    assert!(on(View::Taken).contains(&MarkKind::Defensive));
    assert!(on(View::Taken).contains(&MarkKind::ActiveMitigation));
    assert!(!on(View::Taken).contains(&MarkKind::Cooldown));
    assert!(!on(View::Taken).contains(&MarkKind::SupportBuff));
    assert!(!on(View::Taken).contains(&MarkKind::HealingCooldown));

    let role = role_marked();
    let taken = for_view(&role, View::Taken);
    assert_eq!(taken.buckets, role.buckets);
    assert_eq!(taken.bucket_ms, role.bucket_ms);
    assert!(taken.marks.iter().all(|m| view_draws(View::Taken, m.kind)));
    assert!(taken.marks.iter().any(|m| m.label == "Shield Wall"));
    assert!(taken.marks.iter().all(|m| m.label != "Combustion"));
    let healing = for_view(&role, View::Healing);
    assert!(healing.marks.iter().any(|m| m.label == "Apotheosis"));
    assert!(healing.marks.iter().all(|m| m.label != "Shield Wall"));

    // A Healthstone stays off the damage graph alone; other consumables
    // and every other view are untouched.
    let stone = Mark {
        at_ms: 1_000,
        kind: MarkKind::Consumable,
        label: "Healthstone".into(),
        spell_id: HEALTHSTONE,
        dur_ms: 0,
        src: String::new(),
        open: false,
    };
    let potion = Mark {
        label: "Potion of Unwavering Focus".into(),
        spell_id: 431_932,
        ..stone.clone()
    };
    assert!(!view_draws_mark(View::Damage, &stone));
    assert!(view_draws_mark(View::Damage, &potion));
    for view in [View::Healing, View::Taken, View::EnemyTaken, View::Deaths] {
        assert!(view_draws_mark(view, &stone), "{view:?}");
    }
    let mut with_stone = role.clone();
    with_stone.marks.push(stone.clone());
    let dmg = for_view(&with_stone, View::Damage);
    assert!(dmg.marks.iter().all(|m| m.label != "Healthstone"));
    assert!(for_view(&with_stone, View::Healing).marks.contains(&stone));
    // The legend follows: a Taken graph never keys a cooldown.
    assert!(!kinds_shown(&[&taken], (0, 10)).contains(&MarkKind::Cooldown));
    // A side narrows both of its curves and keeps everything else.
    let side = CompareSide {
        guid: "Player-1-0A".to_string(),
        timeline: role.clone(),
        spell_timeline: Some(role.clone()),
        ..CompareSide::default()
    };
    let narrowed = side_for_view(&side, View::Damage);
    assert_eq!(narrowed.guid, side.guid);
    assert!(
        narrowed
            .timeline
            .marks
            .iter()
            .all(|m| m.label != "Shield Wall")
    );
    let focus = narrowed.spell_timeline.expect("the focus curve survives");
    assert!(focus.marks.iter().all(|m| m.label != "Shield Wall"));
    assert!(focus.marks.iter().any(|m| m.label == "Combustion"));
}

const W: f32 = 200.0;

fn plot_of(t: &Timeline, view: (usize, usize)) -> Plot {
    Plot::new(
        t,
        GraphMode::Dps,
        peak_of(&[t], GraphMode::Dps, view),
        view,
        Vec::new(),
    )
}

#[test]
fn graph_geometry_maps_buckets_to_pixels_and_back() {
    let t = marked();
    let g = plot_of(&t, (0, 10));
    assert_eq!(g.span(), 10.0);
    assert_eq!(g.x_of(0.0, W), 0.0);
    assert_eq!(g.x_of(5.0, W), W / 2.0);
    assert_eq!(g.ms_at(0.0, W), 0);
    assert_eq!(g.ms_at(W / 2.0, W), 5_000);
    assert_eq!(g.ms_at(W * 3.0, W), 10_000, "clamped to the edge");
    assert_eq!(g.mark_x(&t.marks[0], W), W * 0.2);
    assert!(g.mark_visible(&t.marks[0]));
    assert_eq!(g.probe_at(0.0, W), Some((0, g.points[0])));
    assert_eq!(
        g.probe_at(W, W),
        Some((9, g.points[9])),
        "clamped to the last bucket"
    );

    // Zoomed to buckets 4..8: the use at 2s is off-screen, the proc at
    // 7s is three quarters across.
    let z = plot_of(&t, (4, 8));
    assert_eq!(z.span(), 4.0);
    assert!(!z.mark_visible(&t.marks[0]));
    assert!(z.mark_visible(&t.marks[1]));
    assert_eq!(z.mark_x(&t.marks[1], W), W * 0.75);
    assert_eq!(z.ms_at(0.0, W), 4_000);
    assert_eq!(z.probe_at(0.0, W).map(|(b, _)| b), Some(4));
    assert_eq!(z.probe_at(W, W).map(|(b, _)| b), Some(7));

    // Empty curve: nothing to probe.
    let e = plot_of(&timeline(Vec::new()), (0, 1));
    assert_eq!(e.probe_at(50.0, W), None);
}

#[test]
fn the_icon_band_finds_the_nearest_marker_only() {
    let t = marked();
    let g = plot_of(&t, (0, 10));
    let proc_x = g.mark_x(&t.marks[1], W);
    assert_eq!(
        g.mark_at(proc_x + 3.0, 5.0, W).map(|m| m.label.as_str()),
        Some("Proc")
    );
    assert_eq!(
        g.mark_at(proc_x, ICON_BAND + 1.0, W),
        None,
        "below the band"
    );
    assert_eq!(
        g.mark_at(proc_x + 30.0, 5.0, W),
        None,
        "too far from any icon"
    );
    let use_x = g.mark_x(&t.marks[0], W);
    assert_eq!(
        g.mark_at(use_x, 1.0, W).map(|m| m.label.as_str()),
        Some("Trinket")
    );
}

/// The curve stands on its floor and peaks under the icon band, and a
/// marker's line stops at the higher of the curve and the ghost.
#[test]
fn values_map_to_heights_under_the_band_and_marks_hang_on_the_curve() {
    let t = timeline(vec![0, 50, 100]);
    let mut g = Plot::new(&t, GraphMode::Total, 150.0, (0, 3), Vec::new());
    let h = 100.0;
    assert_eq!(g.floor(1.0), 0.0, "no lane, no lift");
    assert_eq!(g.y_of(0.0, h, 0.0), h);
    // The top is the band and two icons (52) capped at half the height.
    assert_eq!(g.y_of(150.0, h, 0.0), 50.0);
    assert_eq!(g.y_of(75.0, h, 0.0), 75.0);
    g.peak = 0.0;
    assert_eq!(g.y_of(1e9, h, 4.0), h - 4.0, "a zero peak pins the floor");

    g.spans = vec![(0, 1_000)];
    assert_eq!(Plot::lane_h(2.0), 4.0);
    assert_eq!(g.floor(2.0), 8.0, "the lane and its gap");

    let g = Plot::new(&t, GraphMode::Total, 150.0, (0, 3), Vec::new());
    let at = |ms: i64| mark(ms, MarkKind::TrinketUse, "T", 0);
    // Cumulative 0, 50, 150: at 1s the curve is at 50.
    let own = g.hang_y(&at(1_000), None, h, 0.0);
    assert_eq!(own, g.y_of(50.0, h, 0.0));
    let ghost = [0.0, 150.0, 0.0];
    assert_eq!(
        g.hang_y(&at(1_000), Some(&ghost), h, 0.0),
        g.y_of(150.0, h, 0.0),
        "the higher curve wins"
    );
    assert_eq!(g.hang_y(&at(9_000), None, h, 0.0), h, "past the curve");
}

#[test]
fn a_drag_is_a_window_and_a_wander_is_a_click() {
    let g = plot_of(&marked(), (0, 10));
    assert_eq!(g.drag_range(50.0, 150.0, W), Some((2_500, 7_500)));
    assert_eq!(
        g.drag_range(150.0, 50.0, W),
        Some((2_500, 7_500)),
        "backwards still lo < hi"
    );
    assert_eq!(g.drag_range(50.0, 52.9, W), None);
    assert!(
        g.drag_range(50.0, 53.0, W).is_some(),
        "the threshold is a drag"
    );
    assert_eq!(g.drag_range(50.0, 60.0, W), Some((2_500, 3_000)));
}

#[test]
fn a_comparison_words_its_tables_and_its_wait() {
    assert_eq!(table_words(View::Taken), ("hit by", "nothing landed"));
    assert_eq!(table_words(View::Damage), ("spell", "no damage recorded"));
    assert_eq!(table_words(View::Dispels).0, "dispel");
    assert_eq!(waiting_words(&[]), "pick two players to compare");
    let one = [("Player-1".to_string(), "Thraxx-Nebula-US".to_string())];
    assert_eq!(waiting_words(&one), "comparing Thraxx — pick one more");
    let two = [one[0].clone(), one[0].clone()];
    assert_eq!(waiting_words(&two), "loading comparison…");
}
