//! Layout D of the design study: what a Taken drill says about the player
//! beyond their lists — the R21 stack ledger as a matrix (rows: the
//! abilities that hit them; columns: the open debuff's stack level; cells:
//! the average hit), heat-shaded so reading across a row is the whole
//! ruling — how much worse does this get per stack — and the R9 death
//! windows as chips. (The R17 mitigation record is the inspector's line
//! now.) Everything here is pure over the wire fields and message-generic.

use iced::widget::{Space, column, container, row, text};
use iced::{Color, Element, Length, Theme};

use wowdps_model::fmt::{duration, human};

use crate::theme::{self, Density, DensityPitch, size};

// The matrices' derivation and their heat are gui-logic's
// (`inspect::matrix`); this module draws them.
use wowdps_gui_logic::inspect::matrix::{self as derive, FOOT, dropped_words};
pub(crate) use wowdps_gui_logic::inspect::matrix::{Matrix, matrices};

/// Heat between green (the row's smallest average) and red (its largest),
/// through amber (`matrix::heat`, in iced's colour).
fn heat(t: f32) -> Color {
    theme::c(derive::heat(t, &wowdps_gui_logic::theme::GOLD.window))
}

const GAP: f32 = 6.0;
const LEVEL_W: f32 = 56.0;
const HITS_W: f32 = 44.0;

/// The matrices drawn: a titled table per debuff, `None` when the ledger
/// is empty (no debuff was open while a hit landed).
pub(crate) fn stack_matrix<M: 'static>(
    matrices: &[Matrix],
    dropped: u32,
) -> Option<Element<'static, M>> {
    if matrices.is_empty() {
        return None;
    }
    let cell = |s: String, color: Color, w: f32| {
        text(s)
            .size(size::MICRO)
            .color(color)
            .font(theme::UI)
            .width(Length::Fixed(w))
            .align_x(iced::Alignment::End)
    };
    let mut body = column![].spacing(8);
    for m in matrices {
        let mut head = row![
            text(format!("{} · stacks", m.aura))
                .size(size::SMALL)
                .color(theme::INK)
                .font(theme::UI_SEMIBOLD)
                .width(Length::Fill),
        ]
        .spacing(GAP);
        for level in 0..=m.max_level {
            head = head.push(cell(level.to_string(), theme::GOLD_DIM, LEVEL_W));
        }
        head = head.push(cell("hits".to_string(), theme::GOLD_DIM, HITS_W));
        let mut table = column![head].spacing(2);
        for r in &m.rows {
            let hits = r.hits();
            let mut line = row![
                text(r.label.clone())
                    .size(size::MICRO)
                    .color(theme::INK)
                    .width(Length::Fill),
            ]
            .spacing(GAP);
            for c in &r.cells {
                line = line.push(match c {
                    Some((_, avg)) => cell(human(*avg), heat(r.heat_t(*avg)), LEVEL_W),
                    None => cell("—".to_string(), theme::INK_3, LEVEL_W),
                });
            }
            line = line.push(cell(hits.to_string(), theme::INK_2, HITS_W));
            table = table.push(line);
        }
        body = body.push(table);
    }
    let mut foot = row![text(FOOT).size(size::TINY).color(theme::INK_2)].spacing(8);
    if let Some(words) = dropped_words(dropped) {
        foot = foot.push(text(words).size(size::TINY).color(theme::INK));
    }
    body = body.push(foot);
    Some(
        container(body)
            .padding(Density::Comfortable.pad())
            .width(Length::Fill)
            .style(|_: &Theme| crate::nav::surface_style(8.0))
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
            .color(theme::INK_2)
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    if dropped > 0 {
        strip = strip.push(
            text(format!("{dropped} older not kept"))
                .size(size::TINY)
                .color(theme::INK_2),
        );
    }
    for d in deaths {
        let on = shown == Some(d.index);
        // A death is a person's moment on the clock: the chip is the
        // window's own, lit by the accent's edge when it is the one shown,
        // led by the Deaths view's own skull.
        let ink = if on { theme::INK } else { theme::INK_2 };
        let chip = crate::nav::chip_around(
            row![
                crate::line_icons::line_icon::<M>(crate::line_icons::LineIcon::Skull, 13.0, ink),
                text(duration(d.at_ms))
                    .size(size::MICRO)
                    .color(ink)
                    .wrapping(iced::widget::text::Wrapping::None),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
            on,
            accent,
        );
        strip = strip.push(iced::widget::mouse_area(chip).on_press(on_pick(d.index)));
    }
    strip = strip.push(Space::new().width(Length::Fill));
    strip = strip.push(text("← → step").size(size::TINY).color(theme::INK_3));
    Some(strip.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::testkit::{render, simulator};

    use crate::theme::AMBER;
    use wowdps_gui_logic::inspect::matrix::samples::ledger;

    #[test]
    fn the_matrix_renders_and_heat_runs_green_to_red() {
        let (d, c, b) = ledger();
        let m = matrices(&d, &c, &b);
        let mut ui = simulator(stack_matrix::<()>(&m, 3).unwrap());
        assert!(ui.find("Crushing Smash · stacks").is_ok());
        assert!(ui.find("Tectonic Strike").is_ok());
        assert!(ui.find("500").is_ok());
        assert!(ui.find("3 hits past the cell cap").is_ok());
        let _ = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
        assert!(stack_matrix::<()>(&[], 0).is_none());
        assert_eq!(heat(0.0), theme::GOOD);
        assert_eq!(heat(1.0), theme::BAD);
        let mid = heat(0.5);
        assert!((mid.r - AMBER.r).abs() < 0.01 && (mid.g - AMBER.g).abs() < 0.01);
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
        assert!(ui.find("1:04").is_ok());
        ui.click("2:38").unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![M::Pick(1)]);
        assert!(death_chips(&deaths[..1], Some(0), 0, theme::NEUTRAL, M::Pick).is_none());
        let _ = render(death_chips(&deaths, None, 0, theme::NEUTRAL, M::Pick).unwrap());
    }

    /// The matrices and the chips as gui-new's `inspector_plot_shots` draws
    /// them — the ledger sample, three deaths with the last shown and one
    /// dropped — on the inspector's surface, 16 px around, so the two sets
    /// diff pixel for pixel.
    ///
    /// `WOWDPS_SHOTS_DIR=/tmp/s cargo test -p wowdps-gui taken_shots -- --ignored`
    #[test]
    #[ignore = "writes PNGs; run by hand beside gui-new's"]
    fn taken_shots() {
        use crate::window::testkit::simulator_as;
        use wowdps_gui_logic::inspect::geometry::samples::WIDTH;
        use wowdps_proto::DeathWindow;
        const PAD: f32 = 16.0;
        let Some(dir) = std::env::var_os("WOWDPS_SHOTS_DIR").map(std::path::PathBuf::from) else {
            return;
        };
        let (d, c, b) = ledger();
        let deaths: Vec<DeathWindow> = [64_000, 158_000, 231_000]
            .into_iter()
            .enumerate()
            .map(|(i, at_ms)| DeathWindow {
                index: i as u32,
                at_ms,
            })
            .collect();
        let shots: [(&str, Element<'static, ()>, f32); 2] = [
            (
                "matrix",
                stack_matrix(&matrices(&d, &c, &b), 3).unwrap(),
                150.0,
            ),
            (
                "chips",
                death_chips(&deaths, Some(2), 1, theme::GOLD_ACCENT, |_| ()).unwrap(),
                64.0,
            ),
        ];
        for (name, el, h) in shots {
            let page = container(el)
                .padding(PAD)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(|_| iced::widget::container::Style {
                    background: Some(theme::SURFACE.into()),
                    ..Default::default()
                });
            let frame = iced::Size::new(WIDTH + 2.0 * PAD, h);
            let mut ui = simulator_as(crate::window::settings(), frame, page.into());
            let snap = ui.snapshot(&iced::Theme::TokyoNight).unwrap();
            let scratch = dir.join(".render");
            let _ = std::fs::remove_dir_all(&scratch);
            assert!(snap.matches_image(scratch.join(name)).unwrap());
            let made = std::fs::read_dir(&scratch)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            std::fs::rename(made, dir.join(format!("{name}.png"))).unwrap();
            let _ = std::fs::remove_dir(&scratch);
        }
    }
}
