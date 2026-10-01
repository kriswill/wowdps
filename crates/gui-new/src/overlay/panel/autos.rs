//! The overlay's debug aids, as the iced overlay's (`docs/tracing.md`):
//! `WOWDPS_OVERLAY_DEBUG` traces input on stderr, stamped since start, and
//! the AUTO* aids drive the overlay into a state on outputs nothing can
//! click — a headless compositor, a screenshot rig.
//!
//! - `AUTOSEG=<pos>` parks the frame on that combined-list position once
//!   the list arrives; the other aids hold their fire until it has landed.
//! - `AUTOVIEW=deaths` (`damage`, `healing`, `interrupts`, `cc`, `dispels`,
//!   `taken`, `enemy`) starts on that view (`start_view`, read before the
//!   session's first Watch).
//! - `AUTODRILL=1` opens the top row's drill as soon as there is one; `=2`
//!   descends once more into its top ability.
//! - `AUTOCOMPARE=1` picks the top two rows (the comparison); `=half` the
//!   top one (the badged-but-waiting meter).
//! - `AUTOTOGGLE=1` flips the panel once, about two seconds in.

use std::time::{Duration, Instant};

use gpui_kit::Context;
use wowdps_model::{Action, View};

use super::Overlay;

/// What the environment asked for, and what is still to do.
#[derive(Debug, Default)]
pub struct Autos {
    seg: Option<usize>,
    drill: u8,
    compare: Option<usize>,
}

impl Autos {
    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(format!("WOWDPS_OVERLAY_{k}")).ok();
        Self {
            seg: var("AUTOSEG").and_then(|v| v.parse().ok()),
            drill: match var("AUTODRILL").as_deref() {
                Some("2") => 2,
                Some(_) => 1,
                None => 0,
            },
            compare: var("AUTOCOMPARE").map(|v| if v == "half" { 1 } else { 2 }),
        }
    }
}

/// `WOWDPS_OVERLAY_DEBUG`: trace input on stderr.
pub fn debug() -> bool {
    std::env::var_os("WOWDPS_OVERLAY_DEBUG").is_some()
}

/// One trace line, stamped with the milliseconds since `since`.
pub fn trace(since: Instant, what: std::fmt::Arguments) {
    if debug() {
        eprintln!(
            "overlay: [{:>8.1}ms] {what}",
            since.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// `WOWDPS_OVERLAY_AUTOVIEW`: the view to start on.
pub fn start_view() -> Option<View> {
    match std::env::var("WOWDPS_OVERLAY_AUTOVIEW").ok()?.as_str() {
        "damage" => Some(View::Damage),
        "healing" => Some(View::Healing),
        "interrupts" => Some(View::Interrupts),
        "cc" => Some(View::CrowdControl),
        "dispels" => Some(View::Dispels),
        "deaths" => Some(View::Deaths),
        "taken" => Some(View::Taken),
        "enemy" => Some(View::EnemyTaken),
        _ => None,
    }
}

impl Overlay {
    /// Advance whatever AUTO* aid is ready, after each round of answers.
    pub(super) fn run_autos(&mut self, cx: &mut Context<Self>) {
        if let Some(pos) = self.autos.seg
            && !self.state(cx).entries().is_empty()
        {
            self.autos.seg = None;
            self.act(|s| s.goto_list_pos(pos), cx);
            return;
        }
        if self.autos.seg.is_some() {
            return;
        }
        if self.autos.drill > 0 {
            let state = self.state(cx);
            let ready = if state.drill.is_none() {
                !state.rows().is_empty()
            } else if state.drill_spell().is_none() {
                !state.breakdown().0.is_empty()
            } else {
                self.autos.drill = 0;
                false
            };
            if ready {
                self.autos.drill -= 1;
                self.act(
                    |s| {
                        let mut reqs = if s.drill.is_none() {
                            s.select_row(0)
                        } else {
                            Vec::new()
                        };
                        reqs.extend(s.apply(Action::Open));
                        reqs
                    },
                    cx,
                );
            }
        }
        if let Some(n) = self.autos.compare
            && self.state(cx).rows().len() >= 2
        {
            self.autos.compare = None;
            let picks: Vec<(String, String)> = self
                .state(cx)
                .rows()
                .into_iter()
                .take(n)
                .map(|r| (r.key, r.label))
                .collect();
            self.act(
                |s| {
                    picks
                        .iter()
                        .flat_map(|(key, label)| s.toggle_compare(key, label))
                        .collect()
                },
                cx,
            );
        }
    }

    /// `WOWDPS_OVERLAY_AUTOTOGGLE`: flip the panel once, two seconds in.
    pub(super) fn auto_toggle(&mut self, cx: &mut Context<Self>) {
        if std::env::var_os("WOWDPS_OVERLAY_AUTOTOGGLE").is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |o, cx| o.toggle(cx));
        })
        .detach();
    }
}
