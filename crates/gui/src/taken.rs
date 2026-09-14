//! Layout D of the design study: what a Taken drill says about the player
//! beyond the two panes — the R17 mitigation record as cards and miss
//! chips instead of one sentence, and the R21 stack ledger as a matrix
//! (rows: the abilities that hit them; columns: the open debuff's stack
//! level; cells: the average hit), heat-shaded so reading across a row is
//! the whole ruling — how much worse does this get per stack. Everything
//! here is pure over the wire fields and message-generic.

use iced::widget::{Space, column, container, row, text};
use iced::{Border, Color, Element, Font, Length, Theme};

use wowdps_model::fmt::{duration, human};
use wowdps_model::{MissKind, Mitigation, StackBase, StackCell, StackingDebuff};

use crate::nav;
use crate::theme::{self, DIM, Density, GREEN, RED, YELLOW, size};

/// The five cards over a Taken drill. `taken` is the player's Taken row
/// amount (absorbs included) and `duration_ms` the fight's, for dtps.
pub(crate) fn mitigation_cards(m: &Mitigation, taken: u64, duration_ms: i64) -> Vec<nav::Stat> {
    let secs = (duration_ms.max(1) as f64) / 1000.0;
    let mut cards = vec![
        nav::Stat {
            label: "dtps".to_string(),
            value: human((taken as f64 / secs) as u64),
            sub: Some(format!("{} taken", human(taken))),
            value_color: None,
            headline: true,
        },
        nav::Stat {
            label: "mitigated".to_string(),
            value: format!("{:.0}%", m.mitigated_pct(taken)),
            sub: Some("of everything swung".to_string()),
            value_color: Some(GREEN),
            headline: false,
        },
        nav::Stat {
            label: "absorbed".to_string(),
            value: human(m.absorbed),
            sub: (m.blocked > 0).then(|| format!("blocked {}", human(m.blocked))),
            value_color: None,
            headline: false,
        },
        nav::Stat {
            label: "prevented".to_string(),
            value: human(m.prevented()),
            sub: Some("full absorbs + blocks".to_string()),
            value_color: None,
            headline: false,
        },
    ];
    // R22: a Brewmaster's pair — staggered, and the share re-dealt to
    // themselves, which their Damage row never shows.
    if m.stagger > 0 || m.stagger_ticked > 0 {
        cards.push(nav::Stat {
            label: "staggered".to_string(),
            value: human(m.stagger),
            sub: (m.stagger_ticked > 0).then(|| format!("{} ticked", human(m.stagger_ticked))),
            value_color: Some(YELLOW),
            headline: false,
        });
    }
    cards
}

/// The miss chips: one per kind that happened, in the log's own order.
/// `None` when nothing missed — a row of zeros is not a fact.
pub(crate) fn miss_chips<M: 'static>(m: &Mitigation) -> Option<Element<'static, M>> {
    if m.misses() == 0 {
        return None;
    }
    let mut strip = row![
        text(format!("{} misses", m.misses()))
            .size(size::TINY)
            .color(DIM)
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    for kind in MissKind::ALL {
        let n = m.misses_of(kind);
        if n == 0 {
            continue;
        }
        strip = strip.push(
            container(
                row![
                    text(kind.name()).size(size::MICRO).color(Color::WHITE),
                    text(n.to_string())
                        .size(size::MICRO)
                        .color(DIM)
                        .font(Font::MONOSPACE),
                ]
                .spacing(4),
            )
            .padding([1, 7])
            .style(|_: &Theme| container::Style {
                background: Some(theme::PANEL.into()),
                border: Border {
                    color: theme::RULE,
                    width: 1.0,
                    radius: 8.into(),
                },
                ..container::Style::default()
            }),
        );
    }
    Some(strip.into())
}

/// One debuff's matrix, derived. Rows are the abilities that hit under it,
/// columns the levels 0..=max; a cell is (hits, average) or `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Matrix {
    pub aura: String,
    pub aura_spell_id: u32,
    pub max_level: u16,
    pub rows: Vec<MatrixRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatrixRow {
    pub label: String,
    /// Index 0 is the DERIVED level 0 (R21: the by-ability baseline minus
    /// every cell of that spell), then one per level 1..=max.
    pub cells: Vec<Option<(u32, u64)>>,
}

/// Fold the ledger into one matrix per debuff. Level 0 is the reader's
/// derivation, per damage spell id and PER DEBUFF: `base − Σ this debuff's
/// cells` — every hit of the spell landed either under this debuff (in
/// exactly one of its cells) or not, so the remainder is exactly its
/// level 0, and a second debuff open at the same time changes nothing.
/// Clamped at nothing rather than negative; a spell with no baseline (a
/// pre-retest snapshot) leaves level 0 empty rather than inventing it.
pub(crate) fn matrices(
    stacking: &[StackingDebuff],
    cells: &[StackCell],
    base: &[StackBase],
) -> Vec<Matrix> {
    stacking
        .iter()
        .map(|d| {
            let mut labels: Vec<(u32, String)> = Vec::new();
            for c in cells.iter().filter(|c| c.aura_spell_id == d.spell_id) {
                if !labels.iter().any(|(id, _)| *id == c.damage_spell_id) {
                    labels.push((c.damage_spell_id, c.damage_label.clone()));
                }
            }
            let rows = labels
                .into_iter()
                .map(|(id, label)| {
                    let mut out: Vec<Option<(u32, u64)>> = vec![None; d.max_level as usize + 1];
                    for c in cells
                        .iter()
                        .filter(|c| c.aura_spell_id == d.spell_id && c.damage_spell_id == id)
                    {
                        if let Some(slot) = out.get_mut(c.level as usize)
                            && c.hits > 0
                        {
                            *slot = Some((c.hits, c.sum / u64::from(c.hits)));
                        }
                    }
                    // Level 0: the unconditioned baseline less this debuff's
                    // own cells for the spell.
                    if let Some(b) = base.iter().find(|b| b.damage_spell_id == id) {
                        let (ch, cs) = cells
                            .iter()
                            .filter(|c| c.damage_spell_id == id && c.aura_spell_id == d.spell_id)
                            .fold((0u32, 0u64), |(h, s), c| (h + c.hits, s + c.sum));
                        let hits = b.hits.saturating_sub(ch);
                        let sum = b.sum.saturating_sub(cs);
                        if hits > 0
                            && let Some(slot) = out.first_mut()
                        {
                            *slot = Some((hits, sum / u64::from(hits)));
                        }
                    }
                    MatrixRow { label, cells: out }
                })
                .collect();
            Matrix {
                aura: d.label.clone(),
                aura_spell_id: d.spell_id,
                max_level: d.max_level,
                rows,
            }
        })
        .filter(|m| !m.rows.is_empty())
        .collect()
}

/// Heat between green (the row's smallest average) and red (its largest),
/// through amber. One color per cell, chosen against the ROW, because the
/// question is "how much worse per stack", not "which ability hits hardest".
fn heat(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (a, b, s) = if t < 0.5 {
        (GREEN, YELLOW, t * 2.0)
    } else {
        (YELLOW, RED, (t - 0.5) * 2.0)
    };
    Color {
        r: a.r + (b.r - a.r) * s,
        g: a.g + (b.g - a.g) * s,
        b: a.b + (b.b - a.b) * s,
        a: 1.0,
    }
}

const GAP: f32 = 6.0;
const LEVEL_W: f32 = 56.0;
const HITS_W: f32 = 44.0;

/// The matrices drawn: a titled table per debuff, `None` when the ledger
/// is empty (no debuff was open while a hit landed).
pub(crate) fn stack_matrix<M: 'static>(
    matrices: &[Matrix],
    dropped: u32,
    accent: theme::Accent,
) -> Option<Element<'static, M>> {
    if matrices.is_empty() {
        return None;
    }
    let cell = |s: String, color: Color, w: f32| {
        text(s)
            .size(size::MICRO)
            .color(color)
            .font(Font::MONOSPACE)
            .width(Length::Fixed(w))
            .align_x(iced::Alignment::End)
    };
    let mut body = column![].spacing(8);
    for m in matrices {
        let mut head = row![
            text(format!("{} · stacks", m.aura))
                .size(size::SMALL)
                .color(accent.heading)
                .width(Length::Fill),
        ]
        .spacing(GAP);
        for level in 0..=m.max_level {
            head = head.push(cell(level.to_string(), DIM, LEVEL_W));
        }
        head = head.push(cell("hits".to_string(), DIM, HITS_W));
        let mut table = column![head].spacing(2);
        for r in &m.rows {
            let avgs: Vec<u64> = r.cells.iter().flatten().map(|(_, avg)| *avg).collect();
            let (lo, hi) = (
                avgs.iter().copied().min().unwrap_or(0),
                avgs.iter().copied().max().unwrap_or(0),
            );
            let hits: u32 = r.cells.iter().flatten().map(|(h, _)| *h).sum();
            let mut line = row![
                text(r.label.clone())
                    .size(size::MICRO)
                    .color(Color::WHITE)
                    .width(Length::Fill),
            ]
            .spacing(GAP);
            for c in &r.cells {
                line = line.push(match c {
                    Some((_, avg)) => {
                        let t = if hi > lo {
                            (*avg - lo) as f32 / (hi - lo) as f32
                        } else {
                            0.0
                        };
                        cell(human(*avg), heat(t), LEVEL_W)
                    }
                    None => cell("—".to_string(), DIM, LEVEL_W),
                });
            }
            line = line.push(cell(hits.to_string(), DIM, HITS_W));
            table = table.push(line);
        }
        body = body.push(table);
    }
    let mut foot = row![
        text("level 0 is derived from the by-ability row; a hit under two debuffs counts in both")
            .size(size::TINY)
            .color(DIM)
    ]
    .spacing(8);
    if dropped > 0 {
        foot = foot.push(
            text(format!("{dropped} hits past the cell cap"))
                .size(size::TINY)
                .color(YELLOW),
        );
    }
    body = body.push(foot);
    Some(
        container(body)
            .padding(Density::Comfortable.pad())
            .width(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::PANEL.into()),
                border: Border {
                    color: theme::RULE,
                    width: 1.0,
                    radius: 4.into(),
                },
                ..container::Style::default()
            })
            .into(),
    )
}

/// v28 (R9): the death navigator — one chip per death window, labelled with
/// its clock time, the described one lit; `dropped` older windows are
/// named in words. `on_pick` carries the window's index.
pub(crate) fn death_chips<M: Clone + 'static>(
    deaths: &[wowdps_proto::DeathWindow],
    shown: Option<u32>,
    dropped: u32,
    accent: theme::Accent,
    on_pick: impl Fn(u32) -> M,
) -> Option<Element<'static, M>> {
    if deaths.len() < 2 && dropped == 0 {
        return None;
    }
    let mut strip = row![
        text(format!("{} deaths", deaths.len() + dropped as usize))
            .size(size::TINY)
            .color(DIM)
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if dropped > 0 {
        strip = strip.push(
            text(format!("{dropped} older not kept"))
                .size(size::TINY)
                .color(DIM),
        );
    }
    for d in deaths {
        let on = shown == Some(d.index);
        let chip = container(
            text(format!("⚰ {}", duration(d.at_ms)))
                .size(size::MICRO)
                .color(if on { accent.ink } else { Color::WHITE })
                .font(Font::MONOSPACE),
        )
        .padding([1, 7])
        .style(move |_: &Theme| container::Style {
            background: Some(if on {
                theme::accent_fill(accent)
            } else {
                theme::PANEL.into()
            }),
            border: Border {
                color: theme::RULE,
                width: if on { 0.0 } else { 1.0 },
                radius: 8.into(),
            },
            ..container::Style::default()
        });
        strip = strip.push(iced::widget::mouse_area(chip).on_press(on_pick(d.index)));
    }
    strip = strip.push(Space::new().width(Length::Fill));
    strip = strip.push(
        text("← → step")
            .size(size::TINY)
            .color(DIM)
            .font(Font::MONOSPACE),
    );
    Some(strip.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{render, simulator};

    fn record() -> Mitigation {
        let mut m = Mitigation {
            absorbed: 9_500_000,
            blocked: 0,
            absorbed_full: 11_900_000,
            blocked_full: 0,
            stagger: 1_600_000,
            stagger_ticked: 400_000,
            misses: [0; MissKind::COUNT],
        };
        m.misses[MissKind::Dodge.index()] = 1;
        m.misses[MissKind::Parry.index()] = 39;
        m.misses[MissKind::Absorb.index()] = 211;
        m
    }

    #[test]
    fn the_cards_word_the_record_and_stagger_only_when_it_happened() {
        let cards = mitigation_cards(&record(), 45_000_000, 600_000);
        assert_eq!(cards[0].label, "dtps");
        assert!(cards[0].headline);
        assert_eq!(cards[0].value, human(75_000));
        assert_eq!(cards[1].label, "mitigated");
        assert_eq!(cards[3].value, human(11_900_000));
        assert_eq!(cards[4].label, "staggered");
        assert_eq!(cards[4].sub.as_deref(), Some("400.0k ticked"));
        let plain = Mitigation {
            stagger: 0,
            stagger_ticked: 0,
            ..record()
        };
        assert_eq!(mitigation_cards(&plain, 1, 1).len(), 4, "no stagger card");
    }

    #[test]
    fn miss_chips_name_only_the_kinds_that_happened() {
        let mut ui = simulator(miss_chips::<()>(&record()).unwrap());
        assert!(ui.find("251 misses").is_ok());
        assert!(ui.find("parry").is_ok());
        assert!(ui.find("39").is_ok());
        assert!(ui.find("dodge").is_ok());
        assert!(ui.find("miss").is_err(), "no plain misses happened");
        let none = Mitigation {
            misses: [0; MissKind::COUNT],
            ..record()
        };
        assert!(miss_chips::<()>(&none).is_none());
    }

    fn ledger() -> (Vec<StackingDebuff>, Vec<StackCell>, Vec<StackBase>) {
        let debuff = StackingDebuff {
            spell_id: 100,
            label: "Crushing Smash".into(),
            src: "Boss".into(),
            max_level: 3,
            hits: 6,
        };
        let cell = |level: u16, hits: u32, sum: u64| StackCell {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            aura_spell_id: 100,
            level,
            hits,
            sum,
            max: sum,
        };
        let cells = vec![cell(1, 2, 400), cell(2, 2, 600), cell(3, 2, 1_000)];
        let base = vec![StackBase {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            hits: 10,
            sum: 2_400,
            misses: 1,
        }];
        (vec![debuff], cells, base)
    }

    #[test]
    fn the_matrix_derives_level_zero_and_orders_the_levels() {
        let (d, c, b) = ledger();
        let m = matrices(&d, &c, &b);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].aura, "Crushing Smash");
        assert_eq!(m[0].rows.len(), 1);
        let r = &m[0].rows[0];
        assert_eq!(r.label, "Tectonic Strike");
        // 10 hits / 2 400 total, 6 hits / 2 000 in cells → level 0 is
        // 4 hits averaging 100.
        assert_eq!(
            r.cells,
            vec![
                Some((4, 100)),
                Some((2, 200)),
                Some((2, 300)),
                Some((2, 500))
            ]
        );
        // Without a baseline level 0 stays honestly empty.
        let m = matrices(&d, &c, &[]);
        assert_eq!(m[0].rows[0].cells[0], None);
        // Cells that outnumber the baseline clamp at nothing.
        let mut low = b.clone();
        low[0].hits = 3;
        low[0].sum = 100;
        let m = matrices(&d, &c, &low);
        assert_eq!(m[0].rows[0].cells[0], None);
        // A debuff nothing landed under draws no matrix.
        assert!(matrices(&d, &[], &b).is_empty());
        // A second debuff open over the same hits takes nothing from the
        // first one's level 0: each debuff's remainder is its own.
        let mut two = d.clone();
        two.push(StackingDebuff {
            spell_id: 200,
            label: "Rending Slash".into(),
            src: "Boss".into(),
            max_level: 1,
            hits: 6,
        });
        let mut overlapped = c.clone();
        overlapped.push(StackCell {
            damage_spell_id: 7,
            damage_label: "Tectonic Strike".into(),
            aura_spell_id: 200,
            level: 1,
            hits: 6,
            sum: 2_000,
            max: 500,
        });
        let m = matrices(&two, &overlapped, &b);
        assert_eq!(m.len(), 2);
        assert_eq!(
            m[0].rows[0].cells[0],
            Some((4, 100)),
            "unchanged by the overlap"
        );
        assert_eq!(m[1].rows[0].cells, vec![Some((4, 100)), Some((6, 333))]);
    }

    #[test]
    fn the_matrix_renders_and_heat_runs_green_to_red() {
        let (d, c, b) = ledger();
        let m = matrices(&d, &c, &b);
        let mut ui = simulator(stack_matrix::<()>(&m, 3, theme::NEUTRAL).unwrap());
        assert!(ui.find("Crushing Smash · stacks").is_ok());
        assert!(ui.find("Tectonic Strike").is_ok());
        assert!(ui.find("500").is_ok());
        assert!(ui.find("3 hits past the cell cap").is_ok());
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
        assert!(stack_matrix::<()>(&[], 0, theme::NEUTRAL).is_none());
        assert_eq!(heat(0.0), GREEN);
        assert_eq!(heat(1.0), RED);
        let mid = heat(0.5);
        assert!((mid.r - YELLOW.r).abs() < 0.01 && (mid.g - YELLOW.g).abs() < 0.01);
    }

    #[test]
    fn death_chips_light_the_shown_window_and_hide_for_one_death() {
        use wowdps_proto::DeathWindow;
        #[derive(Debug, Clone, PartialEq)]
        enum M {
            Pick(u32),
        }
        let deaths = vec![
            DeathWindow {
                index: 0,
                at_ms: 64_000,
            },
            DeathWindow {
                index: 1,
                at_ms: 158_000,
            },
            DeathWindow {
                index: 2,
                at_ms: 231_000,
            },
        ];
        let mut ui = simulator(death_chips(&deaths, Some(2), 1, theme::NEUTRAL, M::Pick).unwrap());
        assert!(ui.find("4 deaths").is_ok());
        assert!(ui.find("1 older not kept").is_ok());
        assert!(ui.find("⚰ 1:04").is_ok());
        ui.click("⚰ 2:38").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Pick(1)]);
        assert!(death_chips(&deaths[..1], Some(0), 0, theme::NEUTRAL, M::Pick).is_none());
        let _ = render(death_chips(&deaths, None, 0, theme::NEUTRAL, M::Pick).unwrap());
    }
}
