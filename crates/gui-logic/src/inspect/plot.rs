//! The graph's data: each curve with its colour and how it is inked, the
//! spans the player spent dead, and how a value reads in the hover — what
//! the inspector builds from a snapshot and a canvas draws.

use wowdps_model::fmt::commas;

use crate::table::figure;
use crate::theme::Color;

/// How a curve is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// The player's own: a line over a faint area of its colour.
    Area,
    /// One of a comparison's two.
    Line,
    /// The comparison's second.
    Dashed,
    /// Context behind a focus curve: the player's whole line behind the
    /// drilled ability's.
    Ghost,
    /// R26: one band of a stack — drawn on top of every `Stack` curve before
    /// it in the list, a 2 px gap of the panel between each, its colour
    /// solid (`points` are its OWN values; the draw sums them).
    Stack,
    /// The stack's last band, the rest ("Other"): a `Stack` band a theme may
    /// draw apart — its upper edge is the player's whole curve.
    StackRest,
}

impl Ink {
    /// A band of the stack, the rest's included.
    pub fn is_stack(self) -> bool {
        matches!(self, Ink::Stack | Ink::StackRest)
    }
}

/// One curve: a value per bucket of `bucket_ms`, from the fight's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    /// Who it is, for the hover ("" for a lone curve: the rate word says).
    pub name: String,
    pub color: Color,
    pub points: Vec<f64>,
    pub bucket_ms: u32,
    pub ink: Ink,
}

impl Curve {
    /// The value at `ms` from the fight's start, when the curve has one.
    pub fn at(&self, ms: u32) -> Option<f64> {
        self.points
            .get((ms / self.bucket_ms.max(1)) as usize)
            .copied()
    }
}

/// A span the player spent dead (R23): from the death to the rez, or to
/// the fight's end, and what to say of it ("died 5:45, rezzed by X").
#[derive(Debug, Clone, PartialEq)]
pub struct Dead {
    pub at_ms: i64,
    pub end_ms: i64,
    pub words: String,
}

/// A figure as the hover reads it: a rate whole with its commas, a
/// running total short.
pub fn value_words(v: f64, total: bool) -> String {
    if total {
        figure(v.max(0.0).round() as u64)
    } else {
        commas(v.max(0.0).round() as u64)
    }
}
