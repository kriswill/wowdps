//! R26 (step 2): the drill graph stacked — a player's curve split into the
//! six largest entries of their ability tree (or an open ability's six
//! largest targets), each a band in its own hue, the rest one neutral
//! "Other" band on top, so the bands always sum to the player's own curve.
//!
//! The hues are a categorical palette validated for the window's surface
//! (`#10162A`) by the data-viz method's checks — OKLCH lightness band and
//! chroma floor, adjacent-pair colour-vision separation (worst ΔE 9.4
//! deutan) and normal-vision separation (worst ΔE 19.7), contrast ≥ 3:1 —
//! in the order that passes; bands stack in SLOT order, so every two
//! that touch are an adjacent, validated pair. No ochre (it reads as the
//! interface's gold) and no red (an outcome: a death).
//!
//! A colour follows its entity, never its rank: `Slots` seats each curve's
//! key once and keeps it while the curve stays in the stack, so a live
//! re-sort never repaints the survivors.
//!
//! Window-only.

use crate::theme::{Color, DataTokens};
use wowdps_model::{AbilitySeries, Timeline};

use super::plot;

// The six hues, in the order that passes, and the "Other" band — neutral,
// so it reads as the rest and never as an entity of its own, 3.0:1 on the
// surface, the marks' floor — are the theme's data tokens (`stack_1` …
// `stack_6`, `stack_other`; `navy`: blue, orange, aqua, violet, magenta,
// green).

/// Which slot each stacked curve's key sits in, for one context (a player,
/// a view, an open ability).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slots {
    context: String,
    keys: [Option<String>; 6],
}

impl Slots {
    /// Seat `keys` — a snapshot's stacked curves, largest first — for
    /// `context`: a key already seated keeps its slot, one that left the
    /// stack gives its slot up, a new one takes the lowest free slot; a new
    /// context starts over, largest in the first slot.
    pub fn observe(&mut self, context: &str, keys: &[String]) {
        if self.context != context {
            self.context = context.to_string();
            self.keys = Default::default();
        }
        for slot in &mut self.keys {
            if slot.as_ref().is_some_and(|k| !keys.contains(k)) {
                *slot = None;
            }
        }
        for key in keys {
            if self.keys.iter().any(|s| s.as_ref() == Some(key)) {
                continue;
            }
            if let Some(free) = self.keys.iter_mut().find(|s| s.is_none()) {
                *free = Some(key.clone());
            }
        }
    }

    /// The slot `key` sits in, when seated for `context`.
    pub fn slot(&self, context: &str, key: &str) -> Option<usize> {
        if self.context != context {
            return None;
        }
        self.keys.iter().position(|s| s.as_deref() == Some(key))
    }
}

/// The context a stack's slots belong to.
pub fn context(player: &str, view: wowdps_model::View, spell: Option<&str>) -> String {
    format!("{player}\u{0}{}\u{0}{}", view.index(), spell.unwrap_or(""))
}

/// The hue `key` wears in `context` — its seated slot, else its place
/// among `keys` (a snapshot the window has not seated yet).
pub fn hue(
    slots: &Slots,
    context: &str,
    keys: &[String],
    key: &str,
    data: &DataTokens,
) -> Option<Color> {
    let at = slots
        .slot(context, key)
        .or_else(|| keys.iter().position(|k| k == key))?;
    data.stack().get(at).copied()
}

/// The stacked curves over `whole` — one band per series in slot order,
/// then "Other" (`whole` less every band, when anything is left) — cut to
/// `mode` and `bucket` exactly as the plain curve is. `name` names a
/// series' band for the hover.
#[allow(clippy::too_many_arguments)]
pub fn curves(
    series: &[AbilitySeries],
    whole: &Timeline,
    slots: &Slots,
    context: &str,
    name: impl Fn(&str) -> String,
    cut: impl Fn(&Timeline) -> (Vec<f64>, u32),
    data: &DataTokens,
) -> Vec<plot::Curve> {
    let keys: Vec<String> = series.iter().map(|s| s.key.clone()).collect();
    let mut seated: Vec<(usize, &AbilitySeries)> = series
        .iter()
        .enumerate()
        .map(|(i, s)| (slots.slot(context, &s.key).unwrap_or(i), s))
        .collect();
    seated.sort_by_key(|(slot, _)| *slot);
    let as_timeline = |buckets: Vec<u64>| Timeline {
        bucket_ms: whole.bucket_ms,
        buckets,
        marks: Vec::new(),
    };
    let mut rest = whole.buckets.clone();
    let mut out = Vec::new();
    for (_, s) in seated {
        for (r, b) in rest.iter_mut().zip(&s.buckets) {
            *r = r.saturating_sub(*b);
        }
        let (points, bucket_ms) = cut(&as_timeline(s.buckets.clone()));
        out.push(plot::Curve {
            name: name(&s.key),
            color: hue(slots, context, &keys, &s.key, data).unwrap_or(data.stack_other),
            points,
            bucket_ms,
            ink: plot::Ink::Stack,
        });
    }
    if rest.iter().any(|r| *r > 0) {
        let (points, bucket_ms) = cut(&as_timeline(rest));
        out.push(plot::Curve {
            name: "Other".to_string(),
            color: data.stack_other,
            points,
            bucket_ms,
            ink: plot::Ink::StackRest,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    /// A key keeps its slot through a re-sort; one that leaves frees its
    /// slot for the next newcomer; a new context starts over.
    #[test]
    fn a_colour_follows_its_entity() {
        let mut s = Slots::default();
        s.observe("p", &keys(&["a", "b", "c"]));
        assert_eq!(s.slot("p", "a"), Some(0));
        s.observe("p", &keys(&["c", "a", "b"]));
        assert_eq!(
            (s.slot("p", "a"), s.slot("p", "b"), s.slot("p", "c")),
            (Some(0), Some(1), Some(2)),
            "a live re-sort repaints nobody"
        );
        s.observe("p", &keys(&["c", "a", "d"]));
        assert_eq!(s.slot("p", "d"), Some(1), "b's slot, freed");
        assert_eq!(s.slot("q", "a"), None, "another context");
        s.observe("q", &keys(&["d"]));
        assert_eq!(s.slot("q", "d"), Some(0));
    }

    /// The bands stack in slot order, and "Other" is the whole less them.
    #[test]
    fn the_bands_and_other_sum_to_the_whole() {
        let whole = Timeline {
            bucket_ms: 1000,
            buckets: vec![10, 20, 30],
            marks: Vec::new(),
        };
        let series = vec![
            AbilitySeries {
                key: "big".into(),
                buckets: vec![5, 10, 10],
            },
            AbilitySeries {
                key: "small".into(),
                buckets: vec![1, 2, 3],
            },
        ];
        let mut slots = Slots::default();
        slots.observe("p", &keys(&["small", "big"]));
        let cut = |t: &Timeline| {
            (
                t.buckets.iter().map(|b| *b as f64).collect::<Vec<f64>>(),
                t.bucket_ms,
            )
        };
        let data = crate::theme::NAVY.data;
        let c = curves(
            &series,
            &whole,
            &slots,
            "p",
            |k| k.to_uppercase(),
            cut,
            &data,
        );
        let names: Vec<&str> = c.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["SMALL", "BIG", "Other"], "slot order, Other on top");
        assert_eq!(c[0].color, data.stack_1);
        assert_eq!(c[2].color, data.stack_other);
        assert_eq!(c[2].points, vec![4.0, 8.0, 17.0]);
        assert!(c.iter().all(|c| c.ink.is_stack()));
        assert_eq!(c[2].ink, plot::Ink::StackRest, "Other is the rest");
    }
}
