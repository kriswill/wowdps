//! Tooltip text for talent spells: the game's own `Description_lang` with
//! its `$` variable language substituted from the spell tables, plus the
//! cost / range / cast-time lines the in-game tooltip shows.
//!
//! The substitution is deliberately best-effort. The client's tooltip
//! engine knows the caster's stats, spec and conditionals; we know the
//! static tables. What resolves cleanly is resolved ($s1 effect values,
//! $d durations, $t periods, cross-spell $12345s1 references, ${...}
//! arithmetic, $l singular:plural; picks, $?cond[a][b] takes the first
//! branch, |c…|r color and |T…|t texture markup is stripped); anything
//! else is left in place rather than guessed at. Values read at base
//! values: an effect that scales with a stat has no base points and reads
//! 0, and so does a stat an expression names (`$AP`, `$MHP`, mastery,
//! versatility); a duration inside an expression is its seconds, and a
//! product with a 0 in it is 0 whatever else it holds (`eval_expr`).
//!
//! The encounter journal's texts (`journal`, for gen-rubric) speak the same
//! language read at one difficulty: `$?diff16[a][b]` takes the branch that
//! difficulty would, a `$[!16 …$]` block shows only off the difficulties it
//! names (`$[16 …$]` only on them), and a |H…|h link is its text.

use std::collections::{HashMap, HashSet};

use crate::table::{Csv, Tables, text};

/// One spell's tooltip lines. Empty strings mean "no such line".
#[derive(Debug, Clone, Default)]
pub struct Tip {
    pub desc: String,
    pub cost: String,
    pub range: String,
    pub cast: String,
    /// For multi-rank spells: the description at each rank, values scaled
    /// by the rank (rank 2 of a "+5 sec" talent reads "+10 sec"). Empty
    /// for single-rank spells.
    pub desc_ranks: Vec<String>,
}

/// One spell effect: (index, base points, aura period ms, radius yd).
pub(crate) type Effect = (u32, f64, u32, f64);

/// The spell tables a text is read against: `collect` builds one from the
/// base rows, gen-rubric one per difficulty.
pub struct Ctx {
    /// spell → effects, ascending by index.
    pub(crate) effects: HashMap<u32, Vec<Effect>>,
    /// spell → duration ms (-1 = until cancelled).
    pub(crate) durations: HashMap<u32, i64>,
    pub(crate) names: HashMap<u32, String>,
    /// Raw descriptions of cross-referenced spells, for `$@spelldesc<id>`.
    pub(crate) descs: HashMap<u32, String>,
    /// spell → its description variables (`$dmg=…` definitions), used by
    /// `$<dmg>` and bare `$dmg` references.
    pub(crate) vars: HashMap<u32, HashMap<String, String>>,
    /// spell → (proc chance %, max stacks, proc icd ms) from
    /// SpellAuraOptions, for `$h`, `$u` and `$proccooldown`.
    pub(crate) aura: HashMap<u32, (f64, u32, u32)>,
}

impl Ctx {
    /// The effect for a 1-based `$sN` index; a missing index falls back to
    /// the spell's first effect, which is how the client resolves strays
    /// like `$104317m2` on a one-effect spell.
    fn effect(&self, spell: u32, index: u32) -> Option<&Effect> {
        let list = self.effects.get(&spell)?;
        list.iter()
            .find(|(i, _, _, _)| *i + 1 == index)
            .or_else(|| list.first())
    }

    fn points(&self, spell: u32, index: u32) -> Option<f64> {
        self.effect(spell, index).map(|(_, p, _, _)| *p)
    }

    fn period(&self, spell: u32, index: u32) -> Option<u32> {
        self.effect(spell, index)
            .map(|(_, _, t, _)| *t)
            .filter(|t| *t > 0)
    }

    fn radius(&self, spell: u32, index: u32) -> Option<f64> {
        self.effect(spell, index)
            .map(|(_, _, _, r)| *r)
            .filter(|r| *r > 0.0)
    }

    /// `$oN`: the effect's total over the spell's duration — points per
    /// tick times the tick count for periodic effects, else the points.
    fn over(&self, spell: u32, index: u32) -> Option<f64> {
        let (_, points, period, _) = self.effect(spell, index)?;
        let dur = self.durations.get(&spell).copied().unwrap_or(0);
        if *period > 0 && dur > 0 {
            Some(points * (dur as f64 / f64::from(*period)))
        } else {
            Some(*points)
        }
    }
}

fn num(s: &str) -> f64 {
    s.parse().unwrap_or(0.0)
}

/// Trim a value the way tooltips print them: no trailing zeros, one
/// decimal at most, sign dropped (the prose carries the direction).
fn fmt_value(v: f64) -> String {
    let v = v.abs();
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// "12 sec", "1.5 min", "1 hour"; -1 lasts until cancelled.
fn fmt_duration(ms: i64) -> String {
    if ms < 0 {
        return "until cancelled".to_string();
    }
    let secs = ms as f64 / 1000.0;
    if secs < 60.0 {
        format!("{} sec", fmt_value(secs))
    } else if secs < 3600.0 {
        format!("{} min", fmt_value(secs / 60.0))
    } else {
        format!("{} hr", fmt_value(secs / 3600.0))
    }
}

/// A `${…}` expression's text (substituted) with each duration a `$d`
/// wrote in it ("18 sec", "1.5 min", "1 hr") read as its seconds, as the
/// game reads `$d` inside arithmetic (Agony's `${$s1*$d/$t1*$u}` is "18
/// sec" long, 18 in the sum).
fn seconds_in(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while !rest.is_empty() {
        // A number (not the tail of one), a space, a unit, and no letter
        // after the unit.
        let found = rest.char_indices().find_map(|(at, c)| {
            let before = rest.get(..at)?;
            if !c.is_ascii_digit() || before.ends_with(|p: char| p.is_ascii_digit() || p == '.') {
                return None;
            }
            let from = rest.get(at..)?;
            let digits = from
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(from.len());
            let (number, after) = (from.get(..digits)?, from.get(digits..)?);
            [(" sec", 1.0), (" min", 60.0), (" hr", 3600.0)]
                .into_iter()
                .find_map(|(unit, k)| {
                    let tail = after.strip_prefix(unit)?;
                    if tail.starts_with(|c: char| c.is_ascii_alphabetic()) {
                        return None;
                    }
                    let v: f64 = number.parse().ok()?;
                    Some((at, at + digits + unit.len(), v * k))
                })
        });
        match found.and_then(|(from, to, secs)| Some((rest.get(..from)?, rest.get(to..)?, secs))) {
            Some((head, tail, secs)) => {
                out.push_str(head);
                out.push_str(&fmt_number(secs));
                rest = tail;
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// A number as the arithmetic reads it back: whole, else to the thousandth.
fn fmt_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.3}")
    }
}

/// The stat variables a description's arithmetic may name (spell power,
/// attack power, the most health, a primary stat, mastery, versatility),
/// lower case: each reads 0 at base values, as a value that scales with a
/// stat does everywhere in these tooltips (an effect scaled by attack
/// power has no base points, so its `$s1` reads 0, as the talent viewer
/// shows it).
const STAT_VARS: [&str; 14] = [
    "sp",
    "sps",
    "ap",
    "rap",
    "oap",
    "mhp",
    "str",
    "agi",
    "int",
    "sta",
    "pri",
    "mas",
    "versadmg",
    "@versadmg",
];

/// A value in a `${…}` expression read at base values: a number, or one
/// the tables cannot give (the player's level, an unresolved spell's
/// value, a function over one).
#[derive(Clone, Copy, PartialEq)]
enum Val {
    Num(f64),
    Unknown,
}

/// A `${…}` expression (substituted, its durations in seconds:
/// `seconds_in`) as the game would show it at base values: `+ - * /`
/// with `*` and `/` first, parentheses, a stat variable as 0
/// (`STAT_VARS`), and a product with a 0 in it 0 whatever else it holds
/// (an attack-power-scaled base of 0 times a level formula is 0). None
/// where the value cannot be known (a sum with an unknown in it), on a
/// division by 0, a sign before a number, or anything but arithmetic.
fn eval_expr(s: &str) -> Option<f64> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut at = 0usize;
    let v = expr_sum(&chars, &mut at)?;
    if at != chars.len() {
        return None;
    }
    match v {
        Val::Num(n) => Some(n),
        Val::Unknown => None,
    }
}

fn expr_sum(c: &[char], at: &mut usize) -> Option<Val> {
    let mut v = expr_product(c, at)?;
    while let Some(&op @ ('+' | '-')) = c.get(*at) {
        *at += 1;
        let r = expr_product(c, at)?;
        v = match (v, r) {
            (Val::Num(a), Val::Num(b)) => Val::Num(if op == '+' { a + b } else { a - b }),
            _ => Val::Unknown,
        };
    }
    Some(v)
}

fn expr_product(c: &[char], at: &mut usize) -> Option<Val> {
    let mut v = expr_atom(c, at)?;
    while let Some(&op @ ('*' | '/')) = c.get(*at) {
        *at += 1;
        let r = expr_atom(c, at)?;
        v = match (op, v, r) {
            ('/', _, Val::Num(0.0)) => return None,
            ('*', Val::Num(0.0), _) | ('*', _, Val::Num(0.0)) | ('/', Val::Num(0.0), _) => {
                Val::Num(0.0)
            }
            ('*', Val::Num(a), Val::Num(b)) => Val::Num(a * b),
            ('/', Val::Num(a), Val::Num(b)) => Val::Num(a / b),
            _ => Val::Unknown,
        };
    }
    Some(v)
}

fn expr_atom(c: &[char], at: &mut usize) -> Option<Val> {
    match c.get(*at)? {
        '(' => {
            *at += 1;
            let v = expr_sum(c, at)?;
            (c.get(*at) == Some(&')')).then_some(())?;
            *at += 1;
            Some(v)
        }
        d if d.is_ascii_digit() || *d == '.' => {
            let from = *at;
            while c.get(*at).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                *at += 1;
            }
            let n: String = c.get(from..*at)?.iter().collect();
            n.parse().ok().map(Val::Num)
        }
        // A token the substitution left: a stat variable reads 0; any other
        // (with its arguments or its braces) is a value the tables cannot
        // give.
        '$' => {
            *at += 1;
            // A description variable no definition gave (`$<pvp>`).
            if c.get(*at) == Some(&'<') {
                while c.get(*at).is_some_and(|d| *d != '>') {
                    *at += 1;
                }
                (c.get(*at) == Some(&'>')).then_some(())?;
                *at += 1;
                return Some(Val::Unknown);
            }
            let from = *at;
            if c.get(*at) == Some(&'@') {
                *at += 1;
            }
            while c.get(*at).is_some_and(|d| d.is_ascii_alphanumeric()) {
                *at += 1;
            }
            let word: String = c.get(from..*at)?.iter().collect::<String>().to_lowercase();
            if let Some(open @ ('(' | '{')) = c.get(*at).copied() {
                let close = if open == '(' { ')' } else { '}' };
                let mut depth = 0u32;
                while let Some(&d) = c.get(*at) {
                    *at += 1;
                    if d == open {
                        depth += 1;
                    } else if d == close {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                (depth == 0).then_some(())?;
                return Some(Val::Unknown);
            }
            if word.is_empty() {
                return None;
            }
            Some(if STAT_VARS.contains(&word.as_str()) {
                Val::Num(0.0)
            } else {
                Val::Unknown
            })
        }
        _ => None,
    }
}

/// Evaluate one of the description language's built-in functions over its
/// comma-separated (already substituted) arguments.
fn eval_function(name: &str, args: &str) -> Option<f64> {
    // Split on top-level commas only.
    let mut parts: Vec<String> = vec![String::new()];
    let mut par = 0u32;
    for c in args.chars() {
        match c {
            '(' => par += 1,
            ')' => par = par.saturating_sub(1),
            ',' if par == 0 => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(last) = parts.last_mut() {
            last.push(c);
        }
    }
    // Each argument read as a `${…}` expression is (`eval_expr`).
    let vals: Vec<f64> = parts
        .iter()
        .map(|p| eval_expr(&seconds_in(p.trim())))
        .collect::<Option<Vec<f64>>>()?;
    let arg = |i: usize| vals.get(i).copied();
    match (name, vals.len()) {
        ("abs", 1) => Some(arg(0)?.abs()),
        ("floor", 1) => Some(arg(0)?.floor()),
        ("ceil", 1) => Some(arg(0)?.ceil()),
        ("min", 2) => Some(arg(0)?.min(arg(1)?)),
        ("max", 2) => Some(arg(0)?.max(arg(1)?)),
        ("clamp", 3) => Some(arg(0)?.clamp(arg(1)?, arg(2)?)),
        ("gt", 2) => Some(f64::from(arg(0)? > arg(1)?)),
        ("cond", 3) => Some(if arg(0)? != 0.0 { arg(1)? } else { arg(2)? }),
        _ => None,
    }
}

/// Read a balanced `[...]` starting at `chars[i]` (which must be `[`);
/// returns (content, index past the `]`).
fn bracket(chars: &[char], mut i: usize) -> Option<(String, usize)> {
    if chars.get(i) != Some(&'[') {
        return None;
    }
    i += 1;
    let mut depth = 1;
    let mut out = String::new();
    while let Some(&c) = chars.get(i) {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((out, i + 1));
                }
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    None
}

/// Substitute one description. `spell` anchors the indexless tokens.
fn substitute(text: &str, spell: u32, ctx: &Ctx, depth: u32) -> String {
    substitute_scaled(text, spell, ctx, depth, 1.0)
}

/// Like [`substitute`], with the anchor spell's effect values multiplied
/// by `mult` — how rank N of a multi-rank talent words its numbers.
/// Cross-spell references are never scaled.
fn substitute_scaled(text: &str, spell: u32, ctx: &Ctx, depth: u32, mult: f64) -> String {
    expand(text, spell, ctx, depth, mult, None)
}

/// An encounter journal's text read at a difficulty id: its `$` tokens
/// filled in from `ctx` (that difficulty's rows), its markup gone, no
/// line ending in a space.
pub fn journal(text: &str, spell: u32, ctx: &Ctx, difficulty: u32) -> String {
    polish(expand(text, spell, ctx, 0, 1.0, Some(difficulty)))
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Whether a `$?` condition holds for a journal read at `difficulty`. Its
/// `|`-separated terms are `diffN` (any case, a stray `?` or spaces
/// around, `!` to negate), true on that difficulty; any other term (a
/// spell the reader knows) holds, as a tooltip takes the first branch.
fn holds(cond: &str, difficulty: u32) -> bool {
    cond.split('|').any(|term| {
        let term = term.trim().trim_start_matches('?').trim();
        let (not, term) = match term.strip_prefix('!') {
            Some(t) => (true, t.trim()),
            None => (false, term),
        };
        let lower = term.to_ascii_lowercase();
        match lower.strip_prefix("diff").map(str::parse::<u32>) {
            Some(Ok(n)) => (n == difficulty) != not,
            _ => !term.is_empty(),
        }
    })
}

/// The substitution itself; `journal` is the difficulty a journal text is
/// read at (`None`: a tooltip), which turns on the journal's own markup.
fn expand(
    text: &str,
    spell: u32,
    ctx: &Ctx,
    depth: u32,
    mult: f64,
    journal: Option<u32>,
) -> String {
    if depth > 4 {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut last_number: f64 = f64::NAN;
    let mut i = 0usize;
    while let Some(&c) = chars.get(i) {
        if c == '|' {
            // UI markup, either case: |cAARRGGBB…|r colors, |T…|t
            // textures, |A…|a atlas.
            match chars.get(i + 1) {
                Some('c' | 'C') => {
                    // |cAARRGGBB (hex) or |cnNAME_FONT_COLOR: (named — 'n'
                    // is not a hex digit, so the forms cannot collide).
                    if chars
                        .get(i + 2)
                        .is_some_and(|c| c.eq_ignore_ascii_case(&'n'))
                    {
                        let mut j = i + 3;
                        while j < chars.len() && chars.get(j) != Some(&':') {
                            j += 1;
                        }
                        i = (j + 1).min(chars.len());
                    } else {
                        i += 10; // |c + 8 hex digits
                    }
                    continue;
                }
                Some('r' | 'R') => {
                    i += 2;
                    continue;
                }
                Some('T') | Some('A') => {
                    let close = if chars.get(i + 1) == Some(&'T') {
                        't'
                    } else {
                        'a'
                    };
                    let mut j = i + 2;
                    while j < chars.len()
                        && !(chars.get(j) == Some(&'|')
                            && chars
                                .get(j + 1)
                                .is_some_and(|c| c.eq_ignore_ascii_case(&close)))
                    {
                        j += 1;
                    }
                    i = (j + 2).min(chars.len());
                    continue;
                }
                // A journal's link, |H<link>|h[text]|h: its text, without
                // the brackets a link wears.
                Some('H') if journal.is_some() => {
                    let bar_h = |from: usize| {
                        (from..chars.len())
                            .find(|&j| chars.get(j) == Some(&'|') && chars.get(j + 1) == Some(&'h'))
                    };
                    if let Some(open) = bar_h(i + 2)
                        && let Some(close) = bar_h(open + 2)
                    {
                        let shown: String =
                            chars.get(open + 2..close).unwrap_or(&[]).iter().collect();
                        out.push_str(
                            shown
                                .strip_prefix('[')
                                .and_then(|s| s.strip_suffix(']'))
                                .unwrap_or(&shown),
                        );
                        i = close + 2;
                    } else {
                        out.push(c);
                        i += 1;
                    }
                    continue;
                }
                _ => {
                    out.push(c);
                    i += 1;
                    continue;
                }
            }
        }
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        // A `$` token.
        let start = i;
        i += 1;
        // $$ is a literal dollar.
        if chars.get(i) == Some(&'$') {
            out.push('$');
            i += 1;
            continue;
        }
        // A journal's $[ids …$] shows only on the difficulty ids it names,
        // $[!ids …$] only off them. One left open runs to the text's end,
        // less the `]` its writer closed it with.
        if let Some(difficulty) = journal
            && chars.get(i) == Some(&'[')
        {
            let mut j = i + 1;
            let not = chars.get(j) == Some(&'!');
            if not {
                j += 1;
            }
            let mut ids = Vec::new();
            let mut n = 0u32;
            let mut any = false;
            while let Some(&cc) = chars.get(j) {
                if let Some(d) = cc.to_digit(10) {
                    n = n * 10 + d;
                    any = true;
                } else if cc == ',' || cc == ' ' {
                    if any {
                        ids.push(n);
                    }
                    (n, any) = (0, false);
                } else {
                    break;
                }
                j += 1;
            }
            if any {
                ids.push(n);
            }
            let close = (j..chars.len())
                .find(|&k| chars.get(k) == Some(&'$') && chars.get(k + 1) == Some(&']'));
            let (end, next) = match close {
                Some(k) => (k, k + 2),
                None if chars.last() == Some(&']') => (chars.len() - 1, chars.len()),
                None => (chars.len(), chars.len()),
            };
            if ids.contains(&difficulty) != not {
                let inner: String = chars.get(j..end).unwrap_or(&[]).iter().collect();
                out.push_str(&expand(&inner, spell, ctx, depth + 1, mult, journal));
            }
            i = next;
            continue;
        }
        // ${expr}[.N]
        if chars.get(i) == Some(&'{') {
            let mut j = i + 1;
            let mut depth_b = 1;
            let mut inner = String::new();
            while let Some(&cc) = chars.get(j) {
                match cc {
                    '{' => depth_b += 1,
                    '}' => {
                        depth_b -= 1;
                        if depth_b == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                inner.push(cc);
                j += 1;
            }
            if depth_b != 0 {
                out.push('$');
                continue;
            }
            let mut end = j + 1;
            let mut precision: Option<usize> = None;
            if chars.get(end) == Some(&'.')
                && let Some(d) = chars.get(end + 1).and_then(|c| c.to_digit(10))
            {
                precision = Some(d as usize);
                end += 2;
            }
            let substituted = expand(&inner, spell, ctx, depth + 1, mult, journal);
            // Read as the game would at base values: its durations in
            // seconds, a stat as 0, a product with a 0 in it 0
            // (`eval_expr`); what cannot be known is left as written.
            match eval_expr(&seconds_in(&substituted)) {
                Some(v) => {
                    last_number = v;
                    match precision {
                        Some(p) => {
                            let _ = std::fmt::Write::write_fmt(
                                &mut out,
                                format_args!("{:.*}", p, v.abs()),
                            );
                        }
                        None => out.push_str(&fmt_value(v)),
                    }
                }
                None => out.push_str(&substituted),
            }
            i = end;
            continue;
        }
        // $?cond[a]…[b] — take the first branch, drop the rest; a journal
        // takes the first whose condition holds at its difficulty.
        if chars.get(i) == Some(&'?') {
            let mut j = i + 1;
            while j < chars.len() && chars.get(j) != Some(&'[') {
                j += 1;
            }
            let Some((first, mut k)) = bracket(&chars, j) else {
                out.push('$');
                continue;
            };
            let cond = |from: usize, to: usize| -> String {
                chars.get(from..to).unwrap_or(&[]).iter().collect()
            };
            // (condition, branch); the else branch has none.
            let mut branches = vec![(Some(cond(i + 1, j)), first)];
            // Consume else-chains: ?cond[x] and bare [x].
            loop {
                if chars.get(k) == Some(&'?') {
                    let mut m = k + 1;
                    while m < chars.len() && chars.get(m) != Some(&'[') {
                        m += 1;
                    }
                    match bracket(&chars, m) {
                        Some((b, next)) => {
                            branches.push((Some(cond(k + 1, m)), b));
                            k = next;
                        }
                        None => break,
                    }
                } else if chars.get(k) == Some(&'[') {
                    match bracket(&chars, k) {
                        Some((b, next)) => {
                            branches.push((None, b));
                            k = next;
                        }
                        None => break,
                    }
                } else {
                    break;
                }
            }
            let taken = match journal {
                None => branches.first(),
                Some(d) => branches
                    .iter()
                    .find(|(c, _)| c.as_deref().is_none_or(|c| holds(c, d))),
            };
            if let Some((_, branch)) = taken {
                out.push_str(&expand(branch, spell, ctx, depth + 1, mult, journal));
            }
            i = k;
            continue;
        }
        // $l singular:plural; and $L, $g male:female;
        // $lsing:plur; / $gmale:female; — but ONLY when the `:`…`;` form
        // validates nearby, or `$gt(...)`-style function words starting
        // with these letters would be eaten alive.
        if matches!(chars.get(i), Some('l' | 'L' | 'g' | 'G')) {
            let mut j = i + 1;
            let mut body = String::new();
            while let Some(&cc) = chars.get(j) {
                if cc == ';' || body.len() > 64 {
                    break;
                }
                body.push(cc);
                j += 1;
            }
            if chars.get(j) == Some(&';')
                && let Some((a, b)) = body.split_once(':')
            {
                let plural = last_number.is_finite() && (last_number - 1.0).abs() > 0.01;
                out.push_str(if matches!(chars.get(i), Some('l' | 'L')) && plural {
                    b
                } else {
                    a
                });
                i = j + 1;
                continue;
            }
            // Not the :…; form — fall through to the word handling below.
        }
        // $@spellname12345 / $@spellicon12345
        if chars.get(i) == Some(&'@') {
            let mut j = i + 1;
            let mut word = String::new();
            while let Some(&cc) = chars.get(j) {
                if cc.is_ascii_alphabetic() {
                    word.push(cc);
                    j += 1;
                } else {
                    break;
                }
            }
            let mut id = 0u32;
            let mut any = false;
            while let Some(d) = chars.get(j).and_then(|c| c.to_digit(10)) {
                id = id * 10 + d;
                j += 1;
                any = true;
            }
            if any && word == "spellname" {
                if let Some(name) = ctx.names.get(&id) {
                    out.push_str(name);
                }
                i = j;
                continue;
            }
            // Embed the referenced spell's own (substituted) description.
            if any && matches!(word.as_str(), "spelldesc" | "spelltooltip" | "spellaura") {
                if let Some(text) = ctx.descs.get(&id) {
                    out.push_str(expand(text, id, ctx, depth + 1, 1.0, journal).trim());
                }
                i = j;
                continue;
            }
            if any && word == "spellicon" {
                i = j;
                continue;
            }
            out.push('$');
            continue;
        }
        // $<name>: an explicit description-variable reference.
        if chars.get(i) == Some(&'<') {
            let mut j = i + 1;
            let mut name = String::new();
            while let Some(&cc) = chars.get(j) {
                if cc == '>' {
                    break;
                }
                name.push(cc);
                j += 1;
            }
            if chars.get(j) == Some(&'>')
                && let Some(expr) = ctx.vars.get(&spell).and_then(|m| m.get(&name))
            {
                out.push_str(expand(&expr.clone(), spell, ctx, depth + 1, mult, journal).trim());
                i = j + 1;
                continue;
            }
            out.push('$');
            continue;
        }
        // [cross-spell-id] word [index]
        let mut ref_spell = 0u32;
        let mut has_ref = false;
        while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
            ref_spell = ref_spell * 10 + d;
            i += 1;
            has_ref = true;
        }
        let anchor = if has_ref { ref_spell } else { spell };
        let mut word = String::new();
        let mut j = i;
        while let Some(&cc) = chars.get(j) {
            if cc.is_ascii_alphabetic() {
                word.push(cc);
                j += 1;
            } else {
                break;
            }
        }
        // A bare description-variable reference ($abs) beats the letter
        // tokens; then the built-in functions; then the named tokens; then
        // single letters. Longer unknown words are left untouched rather
        // than half-parsed.
        if !has_ref && let Some(expr) = ctx.vars.get(&spell).and_then(|m| m.get(&word)) {
            out.push_str(expand(&expr.clone(), spell, ctx, depth + 1, mult, journal).trim());
            i = j;
            continue;
        }
        // $abs(expr) and friends: evaluate the (substituted) arguments.
        if !has_ref
            && chars.get(j) == Some(&'(')
            && matches!(
                word.as_str(),
                "abs" | "min" | "max" | "floor" | "ceil" | "clamp" | "cond" | "gt"
            )
        {
            let mut k = j + 1;
            let mut inner = String::new();
            let mut par = 1u32;
            while let Some(&cc) = chars.get(k) {
                match cc {
                    '(' => par += 1,
                    ')' => {
                        par -= 1;
                        if par == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                inner.push(cc);
                k += 1;
            }
            if par == 0 {
                let substituted = expand(&inner, spell, ctx, depth + 1, mult, journal);
                if let Some(v) = eval_function(&word, &substituted) {
                    last_number = v.abs();
                    out.push_str(&fmt_value(v));
                    i = k + 1;
                    continue;
                }
            }
        }
        if word == "proccooldown"
            && let Some((_, _, icd)) = ctx.aura.get(&anchor).filter(|(_, _, icd)| *icd > 0)
        {
            let secs = f64::from(*icd) / 1000.0;
            last_number = secs;
            out.push_str(&fmt_value(secs));
            i = j;
            continue;
        }
        let resolved = if word.chars().count() == 1 {
            let letter = word.chars().next().unwrap_or('?');
            i = j;
            let mut index = 0u32;
            let mut has_index = false;
            while let Some(d) = chars.get(i).and_then(|c| c.to_digit(10)) {
                index = index * 10 + d;
                i += 1;
                has_index = true;
            }
            let index = if has_index { index } else { 1 };
            let scale = if has_ref { 1.0 } else { mult };
            match letter {
                's' | 'S' | 'm' | 'M' | 'w' | 'x' => ctx.points(anchor, index).map(|v| {
                    let v = v * scale;
                    last_number = v.abs();
                    fmt_value(v)
                }),
                'a' | 'A' => ctx.radius(anchor, index).map(|v| {
                    last_number = v;
                    fmt_value(v)
                }),
                'o' | 'O' => ctx.over(anchor, index).map(|v| {
                    let v = v * scale;
                    last_number = v.abs();
                    fmt_value(v)
                }),
                'u' | 'U' => ctx
                    .aura
                    .get(&anchor)
                    .map(|(_, stacks, _)| *stacks)
                    .filter(|s| *s > 0)
                    .map(|s| {
                        last_number = f64::from(s);
                        s.to_string()
                    }),
                'h' | 'H' => ctx
                    .aura
                    .get(&anchor)
                    .map(|(chance, _, _)| *chance)
                    .filter(|c| *c > 0.0)
                    .map(|c| {
                        last_number = c;
                        fmt_value(c)
                    }),
                'd' | 'D' => ctx.durations.get(&anchor).map(|ms| fmt_duration(*ms)),
                // Descriptions write their own unit ("every $t1 sec"), so
                // the period is a bare number of seconds.
                't' | 'T' => ctx.period(anchor, index).map(|ms| {
                    let secs = f64::from(ms) / 1000.0;
                    last_number = secs;
                    fmt_value(secs)
                }),
                _ => None,
            }
        } else {
            i = j;
            None
        };
        match resolved {
            Some(v) => out.push_str(&v),
            // A reference to a spell the tables know nothing about is dead
            // text (a removed spell id baked into an old description):
            // drop the token. Anything else is left exactly as written.
            None if has_ref
                && !ctx.effects.contains_key(&anchor)
                && !ctx.durations.contains_key(&anchor)
                && !ctx.descs.contains_key(&anchor) => {}
            None => {
                for &cc in chars.get(start..i).unwrap_or(&[]) {
                    out.push(cc);
                }
            }
        }
    }
    out
}

/// Column index by name, tolerating either of two spellings.
pub(crate) fn col2(csv: &Csv, a: &str, b: &str) -> Result<usize, String> {
    csv.col(a).or_else(|_| csv.col(b))
}

/// Every spell's raw description: Description wins, the aura text fills in
/// for pure passives.
pub(crate) fn descriptions(tables: &Tables) -> Result<HashMap<u32, String>, String> {
    let spell = tables.get("Spell")?;
    let (s_id, s_desc, s_aura) = (
        spell.col("ID")?,
        spell.col("Description_lang")?,
        spell.col("AuraDescription_lang")?,
    );
    let mut all_descs: HashMap<u32, String> = HashMap::new();
    for r in &spell.rows {
        let Ok(id) = text(r, s_id).parse::<u32>() else {
            continue;
        };
        let desc = text(r, s_desc);
        let text = if desc.is_empty() {
            text(r, s_aura)
        } else {
            desc
        };
        if !text.is_empty() {
            all_descs.insert(id, text.to_string());
        }
    }
    Ok(all_descs)
}

/// Description variables: `$dmg=…` definitions attached per spell,
/// referenced from the text as `$<dmg>` or bare `$dmg`.
pub(crate) fn variables(
    tables: &Tables,
    needed: &HashSet<u32>,
) -> Result<HashMap<u32, HashMap<String, String>>, String> {
    Ok(variables_of(&variable_texts(tables)?, needed))
}

/// Every spell's description variables, unparsed: the set the last of
/// its `SpellXDescriptionVariables` rows names that exists. Read once by
/// a caller that asks `variables_of` for many spell sets.
pub(crate) fn variable_texts(tables: &Tables) -> Result<HashMap<u32, &str>, String> {
    let sdv = tables.get("SpellDescriptionVariables")?;
    let (v_id, v_text) = (sdv.col("ID")?, sdv.col("Variables")?);
    let var_sets: HashMap<u32, &str> = sdv
        .rows
        .iter()
        .filter_map(|r| Some((text(r, v_id).parse().ok()?, text(r, v_text))))
        .collect();
    let sxdv = tables.get("SpellXDescriptionVariables")?;
    let (x_spell, x_vars) = (
        sxdv.col("SpellID")?,
        sxdv.col("SpellDescriptionVariablesID")?,
    );
    let mut texts: HashMap<u32, &str> = HashMap::new();
    for r in &sxdv.rows {
        let Ok(id) = text(r, x_spell).parse::<u32>() else {
            continue;
        };
        let Some(text) = text(r, x_vars)
            .parse::<u32>()
            .ok()
            .and_then(|v| var_sets.get(&v))
        else {
            continue;
        };
        texts.insert(id, text);
    }
    Ok(texts)
}

/// The description variables of the spells in `needed`, parsed, from
/// `variable_texts`.
pub(crate) fn variables_of(
    texts: &HashMap<u32, &str>,
    needed: &HashSet<u32>,
) -> HashMap<u32, HashMap<String, String>> {
    needed
        .iter()
        .filter_map(|id| Some((*id, parse_variables(texts.get(id)?))))
        .collect()
}

/// A substituted text's line endings normalized and its doubled spaces
/// (what a dropped dead-reference token leaves behind) collapsed.
fn polish(s: String) -> String {
    let mut s = s.replace("\r\n", "\n").replace('\r', "\n");
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s
}

/// Parse a SpellDescriptionVariables blob: `$name=expr` per definition,
/// an expr running until the next definition line.
fn parse_variables(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut name: Option<String> = None;
    let mut expr = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let def = trimmed
            .strip_prefix('$')
            .and_then(|rest| rest.split_once('='))
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()));
        if let Some((n, e)) = def {
            if let Some(prev) = name.take() {
                out.insert(prev, expr.trim().to_string());
            }
            name = Some(n.to_string());
            expr = e.to_string();
        } else if name.is_some() {
            expr.push(' ');
            expr.push_str(trimmed);
        }
    }
    if let Some(prev) = name {
        out.insert(prev, expr.trim().to_string());
    }
    out
}

/// Harvest cross-spell ids out of a description's `$`-tokens, so their
/// tables load too. Only digit runs inside a token count — anchored at a
/// `$` and reached through token characters (`$465s2`, `$136t1`,
/// `$@spelldesc465862`, `$?s137001[…]`, `${$s1*2}`) — with NO length
/// floor: 3-digit spell ids are real (`$465s2` is Devotion Aura's), and a
/// digit-length heuristic left their tokens unsubstituted in the output.
/// Effect indexes (`$s1`) harvest tiny spurious ids; harmless, they gate
/// table rows, never the output.
pub fn ids_in(text: &str, out: &mut HashSet<u32>) {
    let mut digits = String::new();
    let mut in_token = false;
    for c in text.chars().chain(std::iter::once(' ')) {
        if c == '$' {
            in_token = true;
            digits.clear();
            continue;
        }
        if in_token && c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if !digits.is_empty()
            && let Ok(id) = digits.parse::<u32>()
        {
            out.insert(id);
        }
        digits.clear();
        in_token = in_token
            && (c.is_ascii_alphabetic()
                || matches!(
                    c,
                    '@' | '?' | '!' | '(' | ')' | '{' | '}' | '.' | '*' | '/' | '+' | '-' | '='
                ));
    }
}

/// The spells a text names, in order: a `$1284506a1` reference to another
/// spell's numbers, a `$@spellname…`, a `|Hspell:…|h` link, and a
/// description it embeds (`$@spelldesc…`, `…tooltip`, `…aura`: `true`
/// beside these, whose own text names more). Unlike [`ids_in`], never an
/// effect index or a condition's spell: only where a spell must stand.
pub fn spell_refs(text: &str) -> Vec<(u32, bool)> {
    let chars: Vec<char> = text.chars().collect();
    let run = |mut j: usize, f: fn(&char) -> bool| {
        let mut s = String::new();
        while let Some(c) = chars.get(j).filter(|c| f(c)) {
            s.push(*c);
            j += 1;
        }
        (s, j)
    };
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        i += 1;
        if c == '|' && chars.get(i) == Some(&'H') {
            let (link, _) = run(i + 1, |c| *c != '|');
            if let Some(id) = link
                .strip_prefix("spell:")
                .and_then(|l| l.split(':').next())
                .and_then(|n| n.parse().ok())
            {
                out.push((id, false));
            }
            continue;
        }
        if c != '$' {
            continue;
        }
        if chars.get(i) == Some(&'@') {
            let (word, j) = run(i + 1, char::is_ascii_alphabetic);
            let (digits, _) = run(j, char::is_ascii_digit);
            let embeds = match word.as_str() {
                "spellname" => false,
                "spelldesc" | "spelltooltip" | "spellaura" => true,
                _ => continue,
            };
            if let Ok(id) = digits.parse() {
                out.push((id, embeds));
            }
            continue;
        }
        let (digits, j) = run(i, char::is_ascii_digit);
        if chars.get(j).is_some_and(char::is_ascii_alphabetic)
            && let Ok(id) = digits.parse()
        {
            out.push((id, false));
        }
    }
    out
}

/// Build tooltip lines for every wanted spell. `ranks` names each spell's
/// talent max-rank; spells ranked above 1 also get per-rank descriptions.
pub fn collect(
    tables: &Tables,
    wanted: &HashSet<u32>,
    names: &HashMap<u32, String>,
    ranks: &HashMap<u32, u32>,
) -> Result<HashMap<u32, Tip>, String> {
    // Every spell's text stays available: `$@spelldesc<id>` embeds other
    // spells' descriptions, and those can reference further spells.
    let all_descs = descriptions(tables)?;
    let raw: HashMap<u32, String> = wanted
        .iter()
        .filter_map(|id| Some((*id, all_descs.get(id)?.clone())))
        .collect();

    let mut needed: HashSet<u32> = wanted.clone();
    let mut level1: HashSet<u32> = HashSet::new();
    for text in raw.values() {
        ids_in(text, &mut level1);
    }
    for id in &level1 {
        if let Some(text) = all_descs.get(id) {
            ids_in(text, &mut needed);
        }
    }
    needed.extend(level1);

    // SpellMisc: duration / range / cast indexes (base difficulty wins).
    let sm = tables.get("SpellMisc")?;
    let (m_spell, m_dur, m_range, m_cast) = (
        sm.col("SpellID")?,
        sm.col("DurationIndex")?,
        sm.col("RangeIndex")?,
        sm.col("CastingTimeIndex")?,
    );
    let m_diff = sm.col("DifficultyID").ok();
    let mut misc: HashMap<u32, (u32, u32, u32)> = HashMap::new();
    for r in &sm.rows {
        let Ok(id) = text(r, m_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) {
            continue;
        }
        let base = m_diff.map(|c| text(r, c)).is_none_or(|d| d == "0");
        if base || !misc.contains_key(&id) {
            misc.insert(
                id,
                (
                    num(text(r, m_dur)) as u32,
                    num(text(r, m_range)) as u32,
                    num(text(r, m_cast)) as u32,
                ),
            );
        }
    }

    // SpellRadius: index → yards (RadiusMax fills in for min/max pairs).
    let sradius = tables.get("SpellRadius")?;
    let (ra_id, ra_r, ra_max) = (
        sradius.col("ID")?,
        sradius.col("Radius")?,
        sradius.col("RadiusMax")?,
    );
    let radius_yd: HashMap<u32, f64> = sradius
        .rows
        .iter()
        .filter_map(|r| {
            let v = num(text(r, ra_r));
            let v = if v > 0.0 { v } else { num(text(r, ra_max)) };
            Some((text(r, ra_id).parse().ok()?, v))
        })
        .collect();

    // SpellEffect: base points, aura periods and radii by effect index.
    let se = tables.get("SpellEffect")?;
    let (e_spell, e_index, e_period) = (
        se.col("SpellID")?,
        se.col("EffectIndex")?,
        se.col("EffectAuraPeriod")?,
    );
    let e_points = col2(se, "EffectBasePointsF", "EffectBasePoints")?;
    let (e_rad0, e_rad1) = (
        se.col("EffectRadiusIndex_0")?,
        se.col("EffectRadiusIndex_1")?,
    );
    let e_diff = se.col("DifficultyID").ok();
    let mut effects: HashMap<u32, Vec<Effect>> = HashMap::new();
    for r in &se.rows {
        let Ok(id) = text(r, e_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) {
            continue;
        }
        if e_diff
            .map(|c| text(r, c))
            .is_some_and(|d| !d.is_empty() && d != "0")
        {
            continue;
        }
        let rad_ix = match num(text(r, e_rad0)) as u32 {
            0 => num(text(r, e_rad1)) as u32,
            ix => ix,
        };
        effects.entry(id).or_default().push((
            num(text(r, e_index)) as u32,
            num(text(r, e_points)),
            num(text(r, e_period)) as u32,
            radius_yd.get(&rad_ix).copied().unwrap_or(0.0),
        ));
    }
    for list in effects.values_mut() {
        list.sort_unstable_by_key(|(i, _, _, _)| *i);
    }

    // Duration / range / cast lookup tables.
    let sd = tables.get("SpellDuration")?;
    let (d_id, d_ms) = (sd.col("ID")?, sd.col("Duration")?);
    let dur_by_index: HashMap<u32, i64> = sd
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                text(r, d_id).parse().ok()?,
                text(r, d_ms).parse::<i64>().ok()?,
            ))
        })
        .collect();

    let sr = tables.get("SpellRange")?;
    let (r_id, r_max0, r_max1) = (sr.col("ID")?, sr.col("RangeMax_0")?, sr.col("RangeMax_1")?);
    let range_by_index: HashMap<u32, f64> = sr
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                text(r, r_id).parse().ok()?,
                num(text(r, r_max0)).max(num(text(r, r_max1))),
            ))
        })
        .collect();

    let sc = tables.get("SpellCastTimes")?;
    let (c_id, c_base) = (sc.col("ID")?, sc.col("Base")?);
    let cast_by_index: HashMap<u32, i64> = sc
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                text(r, c_id).parse().ok()?,
                text(r, c_base).parse::<i64>().ok()?,
            ))
        })
        .collect();

    // SpellPower: the first nonzero cost line.
    let sp = tables.get("SpellPower")?;
    let (p_spell, p_mana, p_pct, p_type) = (
        sp.col("SpellID")?,
        sp.col("ManaCost")?,
        sp.col("PowerCostPct")?,
        sp.col("PowerType")?,
    );
    let mut costs: HashMap<u32, String> = HashMap::new();
    for r in &sp.rows {
        let Ok(id) = text(r, p_spell).parse::<u32>() else {
            continue;
        };
        if !wanted.contains(&id) || costs.contains_key(&id) {
            continue;
        }
        let power = power_name(num(text(r, p_type)) as i64);
        let pct = num(text(r, p_pct));
        let flat = num(text(r, p_mana));
        let line = if pct > 0.0 {
            format!("{}% of base {power}", fmt_value(pct))
        } else if flat > 0.0 {
            // Whole-unit resources store ×10 (shards) or ×100 (insanity).
            format!(
                "{} {power}",
                fmt_value(flat / power_scale(num(text(r, p_type)) as i64))
            )
        } else {
            continue;
        };
        costs.insert(id, line);
    }

    // Durations resolved per spell for $d.
    let durations: HashMap<u32, i64> = misc
        .iter()
        .filter_map(|(spell, (dur, _, _))| Some((*spell, *dur_by_index.get(dur)?)))
        .collect();

    let vars = variables(tables, &needed)?;

    // SpellAuraOptions: proc chance, max stacks, proc cooldown.
    let sao = tables.get("SpellAuraOptions")?;
    let (o_spell, o_chance, o_stacks, o_icd) = (
        sao.col("SpellID")?,
        sao.col("ProcChance")?,
        sao.col("CumulativeAura")?,
        sao.col("ProcCategoryRecovery")?,
    );
    let o_diff = sao.col("DifficultyID").ok();
    let mut aura: HashMap<u32, (f64, u32, u32)> = HashMap::new();
    for r in &sao.rows {
        let Ok(id) = text(r, o_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) || aura.contains_key(&id) {
            continue;
        }
        if o_diff
            .map(|c| text(r, c))
            .is_some_and(|d| !d.is_empty() && d != "0")
        {
            continue;
        }
        aura.insert(
            id,
            (
                num(text(r, o_chance)),
                num(text(r, o_stacks)) as u32,
                num(text(r, o_icd)) as u32,
            ),
        );
    }

    let ctx = Ctx {
        effects,
        durations,
        names: names.clone(),
        descs: all_descs,
        vars,
        aura,
    };

    let mut out = HashMap::new();
    for spell_id in wanted {
        let mut tip = Tip::default();
        if let Some(text) = raw.get(spell_id) {
            tip.desc = polish(substitute(text, *spell_id, &ctx, 0));
            // Rank variants, values scaled by the rank.
            if let Some(max) = ranks.get(spell_id).copied().filter(|m| *m > 1) {
                tip.desc_ranks = (1..=max)
                    .map(|r| polish(substitute_scaled(text, *spell_id, &ctx, 0, f64::from(r))))
                    .collect();
            }
        }
        if let Some(cost) = costs.get(spell_id) {
            tip.cost = cost.clone();
        }
        if let Some((_, range_ix, cast_ix)) = misc.get(spell_id) {
            if let Some(range) = range_by_index.get(range_ix)
                && *range > 5.0
            {
                tip.range = format!("{} yd range", fmt_value(*range));
            }
            match cast_by_index.get(cast_ix) {
                Some(ms) if *ms > 0 => {
                    tip.cast = format!("{} cast", fmt_duration(*ms));
                }
                Some(_) => tip.cast = "Instant".to_string(),
                None => {}
            }
        }
        if !tip.desc.is_empty() || !tip.cost.is_empty() || !tip.range.is_empty() {
            out.insert(*spell_id, tip);
        }
    }
    Ok(out)
}

fn power_name(power_type: i64) -> &'static str {
    match power_type {
        0 => "mana",
        1 => "rage",
        2 => "focus",
        3 => "energy",
        4 => "combo points",
        5 => "runes",
        6 => "runic power",
        7 => "soul shards",
        8 => "astral power",
        9 => "holy power",
        11 => "maelstrom",
        12 => "chi",
        13 => "insanity",
        16 => "arcane charges",
        17 => "fury",
        18 => "pain",
        19 => "essence",
        _ => "power",
    }
}

fn power_scale(power_type: i64) -> f64 {
    match power_type {
        1 | 6 | 17 | 18 => 10.0, // rage-likes store ×10
        7 => 10.0,               // soul shard fragments
        13 => 100.0,             // insanity
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        let mut effects = HashMap::new();
        effects.insert(
            100u32,
            vec![(0u32, -50.0, 0u32, 8.0f64), (1, 12.0, 2000, 0.0)],
        );
        effects.insert(200, vec![(0, 30.0, 0, 0.0)]);
        let mut durations = HashMap::new();
        durations.insert(100u32, 12000i64);
        durations.insert(200, 90000);
        let mut names = HashMap::new();
        names.insert(200u32, "Corruption".to_string());
        let mut descs = HashMap::new();
        descs.insert(200u32, "Deals $s1 damage over $d.".to_string());
        let mut vars = HashMap::new();
        vars.insert(100u32, parse_variables("$dmg=${$s1*2}\n$abs=$s2 flat\n"));
        let mut aura = HashMap::new();
        aura.insert(100u32, (15.0, 3, 8000));
        Ctx {
            effects,
            durations,
            names,
            descs,
            vars,
            aura,
        }
    }

    #[test]
    fn the_common_tokens_resolve() {
        let c = ctx();
        assert_eq!(
            substitute(
                "Reduces the target's movement speed by $s1% for $d.",
                100,
                &c,
                0
            ),
            "Reduces the target's movement speed by 50% for 12 sec."
        );
        assert_eq!(
            substitute("Deals $s2 damage every $t2 sec for $d.", 100, &c, 0),
            "Deals 12 damage every 2 sec for 12 sec."
        );
        // Cross-spell reference and $@spellname.
        assert_eq!(
            substitute("$@spellname200 deals $200s1 over $200d.", 100, &c, 0),
            "Corruption deals 30 over 1.5 min."
        );
        // $@spelldesc embeds the referenced spell's substituted text.
        assert_eq!(
            substitute("Also gain: $@spelldesc200", 100, &c, 0),
            "Also gain: Deals 30 damage over 1.5 min."
        );
    }

    #[test]
    fn arithmetic_conditionals_and_plurals() {
        let c = ctx();
        assert_eq!(
            substitute("gains ${$s1/10} charges", 100, &c, 0),
            "gains 5 charges"
        );
        assert_eq!(
            substitute("$?s137046[Improved: $s1%][Base: $s1%] end", 100, &c, 0),
            "Improved: 50% end"
        );
        assert_eq!(
            substitute("lasts $s2 $lsecond:seconds;", 100, &c, 0),
            "lasts 12 seconds"
        );
        // Markup strips (either case); unknown tokens survive verbatim,
        // except references to spells the tables know nothing about —
        // dead ids in stale text — which drop.
        assert_eq!(
            substitute("|cFFFFFFFFwhite|r scales with $AP power", 100, &c, 0),
            "white scales with $AP power"
        );
        assert_eq!(
            substitute("|CFFE55BB0Soulburn:|R a $104224A yard radius", 100, &c, 0),
            "Soulburn: a  yard radius"
        );
        // Radius tokens, and the missing-index fallback to effect 1
        // (the client's behavior for strays like `$104317m2`).
        assert_eq!(substitute("within $a1 yds", 100, &c, 0), "within 8 yds");
        assert_eq!(
            substitute("summons ${3*$200m2} imps", 100, &c, 0),
            "summons 90 imps"
        );
        // Description variables ($<name> and bare $name), aura data, and
        // $o totals over the duration.
        assert_eq!(substitute("hits for $<dmg>", 100, &c, 0), "hits for 100");
        assert_eq!(substitute("absorbs $abs", 100, &c, 0), "absorbs 12 flat");
        assert_eq!(
            substitute(
                "$h% chance, stacks $u times, $proccooldown sec icd",
                100,
                &c,
                0
            ),
            "15% chance, stacks 3 times, 8 sec icd"
        );
        assert_eq!(
            substitute("deals $o2 over $d", 100, &c, 0),
            "deals 72 over 12 sec"
        );
        // Built-in functions, alone and inside ${} arithmetic. (Spell 100
        // defines an `abs` VARIABLE — the parenthesized function form is
        // exercised on spell 200, which has none.)
        assert_eq!(substitute("by ${$abs($200s1)}%", 200, &c, 0), "by 30%");
        assert_eq!(
            substitute("${$max(5,$200s1)} and ${$cond($gt(2,1),7,9)}", 200, &c, 0),
            "30 and 7"
        );
    }

    #[test]
    fn formatting_rules() {
        assert_eq!(fmt_value(-50.0), "50");
        assert_eq!(fmt_value(2.5), "2.5");
        assert_eq!(fmt_duration(1500), "1.5 sec");
        assert_eq!(fmt_duration(90000), "1.5 min");
        assert_eq!(fmt_duration(-1), "until cancelled");
        assert_eq!(eval_expr("50/10"), Some(5.0));
        assert_eq!(eval_expr("2+3*4"), Some(14.0));
        assert_eq!(eval_expr("50%"), None);
    }

    #[test]
    fn color_markup_strips_in_both_forms() {
        let c = ctx();
        assert_eq!(
            substitute("|cFFFF0000Deals $s2 damage.|r", 100, &c, 0),
            "Deals 12 damage."
        );
        // The named-color form runs to its ':' — a fixed 10-char skip
        // would leave "ONT_COLOR:…" garbage in the tooltip.
        assert_eq!(
            substitute("|cnGREEN_FONT_COLOR:Heals nearby allies.|r", 100, &c, 0),
            "Heals nearby allies."
        );
    }

    #[test]
    fn token_ids_harvest_at_any_length_and_prose_numbers_do_not() {
        let mut out = HashSet::new();
        ids_in("Damage reduction increased to $465s2%.", &mut out);
        assert!(out.contains(&465), "3-digit cross-spell id: {out:?}");
        ids_in(
            "an additional $136d/$136t1% plus $@spelldesc465862",
            &mut out,
        );
        assert!(out.contains(&136) && out.contains(&465862), "{out:?}");
        ids_in("$?s137001[Empowered.][Plain.]", &mut out);
        assert!(out.contains(&137001), "conditional's spell id: {out:?}");
        let mut prose = HashSet::new();
        ids_in("Deals 4000 damage over 10 sec.", &mut prose);
        assert!(
            prose.is_empty(),
            "plain prose numbers are not ids: {prose:?}"
        );
    }

    #[test]
    fn arithmetic_edge_cases() {
        assert_eq!(fmt_duration(7_200_000), "2 hr");
        assert_eq!(fmt_value(0.04), "0");
        // Division by zero, malformed numbers, whitespace, dangling ops.
        assert_eq!(eval_expr("5/0"), None);
        assert_eq!(eval_expr("1.2.3+1"), None);
        assert_eq!(eval_expr("1+1.2.3"), None);
        assert_eq!(eval_expr("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval_expr("10 - 4 - 3"), Some(3.0));
        assert_eq!(eval_expr("+"), None);
        assert_eq!(eval_expr(""), None);
        // Every built-in, plus the arity/name misses.
        assert_eq!(eval_function("abs", "0-3"), Some(3.0));
        assert_eq!(eval_function("floor", "2.7"), Some(2.0));
        assert_eq!(eval_function("ceil", "2.1"), Some(3.0));
        assert_eq!(eval_function("min", "4, 9"), Some(4.0));
        assert_eq!(eval_function("clamp", "15, 1, 10"), Some(10.0));
        assert_eq!(eval_function("gt", "1, 2"), Some(0.0));
        assert_eq!(eval_function("cond", "0, 7, 9"), Some(9.0));
        assert_eq!(eval_function("min", "1, 2, 3"), None);
        assert_eq!(eval_function("sqrt", "4"), None);
        // An argument in parentheses reads as an expression does.
        assert_eq!(eval_function("max", "(1), 2"), Some(2.0));
    }

    #[test]
    fn brackets_and_conditionals() {
        let c = ctx();
        assert_eq!(bracket(&['x'], 0), None);
        assert_eq!(bracket(&['[', 'a', 'b'], 0), None);
        assert_eq!(
            bracket(&['[', 'a', '[', 'b', ']', 'c', ']', 'z'], 0),
            Some(("a[b]c".to_string(), 7))
        );
        // Nested brackets inside the taken branch.
        assert_eq!(substitute("$?s1[a[$s2]c][d]!", 100, &c, 0), "a[12]c!");
        // Else-chains: ?cond[x] and bare [x] alternatives are consumed.
        assert_eq!(substitute("$?s1[A]?s2[B][C] end", 100, &c, 0), "A end");
        // A dangling else-chain stops consuming at the malformed part.
        assert_eq!(substitute("$?s1[A]?s2", 100, &c, 0), "A?s2");
        assert_eq!(substitute("$?s1[A][B", 100, &c, 0), "A[B");
        // No bracket at all, or an unterminated one: the `$` survives.
        assert_eq!(substitute("$?s1", 100, &c, 0), "$?s1");
        assert_eq!(substitute("$?s1[abc", 100, &c, 0), "$?s1[abc");
    }

    #[test]
    fn braces_precision_and_literals() {
        let c = ctx();
        assert_eq!(substitute("costs $$5", 100, &c, 0), "costs $5");
        assert_eq!(substitute("${${$s1}*2}", 100, &c, 0), "100");
        assert_eq!(substitute("${$s1/3}", 100, &c, 0), "16.7");
        assert_eq!(substitute("${$s1/3}.2%", 100, &c, 0), "16.67%");
        assert_eq!(substitute("${$s1/3}.x", 100, &c, 0), "16.7.x");
        // Non-arithmetic contents pass through substituted.
        assert_eq!(substitute("${abc}", 100, &c, 0), "abc");
        // Unterminated brace: the `$` is kept and the rest reparsed.
        assert_eq!(substitute("${$s1", 100, &c, 0), "${50");
        // A function with unbalanced parens, or of the wrong arity, is left
        // as written; one whose argument wears parentheses is worked.
        assert_eq!(substitute("$max(1,2", 200, &c, 0), "$max(1,2");
        assert_eq!(substitute("$min(1,2,3)", 200, &c, 0), "$min(1,2,3)");
        assert_eq!(substitute("$max((1),2)", 200, &c, 0), "2");
    }

    /// An expression reads as the game would show it at base values: a
    /// duration in it in seconds, parentheses, a stat as 0, a product with
    /// a 0 in it 0; what cannot be known stays as written.
    #[test]
    fn expressions_read_at_base_values() {
        let mut c = ctx();
        // Agony: a stat-scaled effect (no base points) ticking every 2 sec
        // for 18 sec, 8 stacks at most.
        c.effects.insert(300, vec![(0, 0.0, 2000, 0.0)]);
        c.durations.insert(300, 18_000);
        c.aura.insert(300, (0.0, 8, 0));
        let at = |s: &str| substitute(s, 300, &c, 0);
        assert_eq!(
            at("causing up to ${$s1*$d/$t1*$u} Shadow damage over $d."),
            "causing up to 0 Shadow damage over 18 sec."
        );
        assert_eq!(at("Restores ${$s1*$d/5} health"), "Restores 0 health");
        assert_eq!(at("in ${$d/$t1+1} waves"), "in 10 waves");
        assert_eq!(at("${$d*2} long"), "36 long");
        // 1.5 min is 90 seconds.
        assert_eq!(substitute("${$200d/3}", 100, &c, 0), "30");
        assert_eq!(at("${(1+15/100)*20}"), "23");
        assert_eq!(at("${100*(1-(100/(100+34)))}%"), "25.4%");
        // A product with a 0 in it, whatever else it holds.
        assert_eq!(at("causes ${$s1*(1+15/100)} damage"), "causes 0 damage");
        assert_eq!(at("${$s1*($max(0,$min($pl-10,10))*11+260)/370}"), "0");
        // An expression left inside one (its spells beyond the tables).
        assert_eq!(
            eval_expr("0*${1+$132158s2/100}*${1+$16870s2/100}"),
            Some(0.0)
        );
        // A stat reads 0, versatility and mastery among them.
        assert_eq!(
            at("absorbing ${35/100*$MHP*(1+$@versadmg)} damage"),
            "absorbing 0 damage"
        );
        assert_eq!(at("${30*(1+$mas/100)}"), "30");
        // What cannot be known, a sign before a number, a division by 0:
        // left as written.
        assert_eq!(at("${$SPS*50/100+$pl} yd"), "$SPS*50/100+$pl yd");
        assert_eq!(substitute("${$s2*-2}%", 100, &c, 0), "12*-2%");
        assert_eq!(at("${$d/0}"), "18 sec/0");
        // A unit's word that runs on is no duration.
        assert_eq!(
            seconds_in("3 seconds, 2 sec, 1.5 min, 1 hr"),
            "3 seconds, 2, 90, 3600"
        );
    }

    #[test]
    fn markup_forms() {
        let c = ctx();
        assert_eq!(
            substitute("|TInterface\\Icons\\foo:20|t Bar", 100, &c, 0),
            " Bar"
        );
        assert_eq!(substitute("|Aatlas:0:0|a x", 100, &c, 0), " x");
        // Unterminated texture markup swallows to the end.
        assert_eq!(substitute("pre |Tunterminated", 100, &c, 0), "pre ");
        // A bare pipe is ordinary text.
        assert_eq!(substitute("a|b", 100, &c, 0), "a|b");
        assert_eq!(substitute("trailing|", 100, &c, 0), "trailing|");
    }

    #[test]
    fn plural_and_gender_picks() {
        let c = ctx();
        // ${…}/50 = 1 -> singular.
        assert_eq!(
            substitute("${$s1/50} $lpoint:points;", 100, &c, 0),
            "1 point"
        );
        // Gender always takes the first form.
        assert_eq!(substitute("$s2 $ghe:she; ok", 100, &c, 0), "12 he ok");
        assert_eq!(substitute("$Ghe:she;", 100, &c, 0), "he");
        // Not the :…; form: `$gt(…)` is a function, `$l` alone is text.
        assert_eq!(substitute("$gt(3,2)", 200, &c, 0), "1");
        assert_eq!(substitute("$lonely", 100, &c, 0), "$lonely");
    }

    #[test]
    fn at_references_and_variables() {
        let c = ctx();
        assert_eq!(substitute("$@spellicon200 x", 100, &c, 0), " x");
        assert_eq!(substitute("$@spellfoo200", 100, &c, 0), "$@spellfoo200");
        assert_eq!(substitute("$@spellname", 100, &c, 0), "$@spellname");
        assert_eq!(substitute("[$@spellname999]", 100, &c, 0), "[]");
        assert_eq!(substitute("[$@spelldesc999]", 100, &c, 0), "[]");
        assert_eq!(
            substitute("$@spelltooltip200|$@spellaura200", 100, &c, 0),
            "Deals 30 damage over 1.5 min.|Deals 30 damage over 1.5 min."
        );
        // Explicit variable references: unknown or unterminated survive.
        assert_eq!(substitute("$<nope>", 100, &c, 0), "$<nope>");
        assert_eq!(substitute("$<abc", 100, &c, 0), "$<abc");
        assert_eq!(substitute("$<dmg>", 200, &c, 0), "$<dmg>");
        // Unknown letters and long words are left verbatim; a known spell's
        // unknown letter too.
        assert_eq!(substitute("$z1 $200z $s", 100, &c, 0), "$z1 $200z 50");
        assert_eq!(substitute("$AP $sTuff", 100, &c, 0), "$AP $sTuff");
        // Aura/period tokens that have no data stay as written.
        assert_eq!(
            substitute("$u $h $t1 $proccooldown", 200, &c, 0),
            "$u $h $t1 $proccooldown"
        );
        // $o on a non-periodic effect is just its points.
        assert_eq!(substitute("$o1", 100, &c, 0), "50");
        assert_eq!(substitute("$o", 999, &c, 0), "$o");
        assert_eq!(substitute("$999d $999s1", 999, &c, 0), " ");
    }

    #[test]
    fn a_journal_reads_its_text_at_a_difficulty() {
        let c = ctx();
        let text = "Bursts within $a1 yds$?diff16|DIFF8[, twice][]$?DIFF17[ (weakly)].\
                    $[!16,8\r\n\r\nOn Mythic, |cFF2959D3|Hspell:200|h[Corruption]|h|r bursts twice.  $]";
        assert_eq!(
            journal(text, 100, &c, 15),
            "Bursts within 8 yds.\n\nOn Mythic, Corruption bursts twice."
        );
        assert_eq!(journal(text, 100, &c, 16), "Bursts within 8 yds, twice.");
        assert_eq!(
            journal(text, 100, &c, 17).lines().next(),
            Some("Bursts within 8 yds (weakly).")
        );
        // $[ids …$] shows only on the ids; one left open runs to the end,
        // less its stray `]`; spaces and a `?` round a condition's terms.
        assert_eq!(journal("A $[ 16 B$]", 100, &c, 16), "A B");
        assert_eq!(journal("A$[16 B$]", 100, &c, 15), "A");
        assert_eq!(journal("A $[!16 B.]", 100, &c, 15), "A B.");
        assert_eq!(journal("$? DIFF23 | DIFF8 [k][n]", 100, &c, 8), "k");
        assert_eq!(journal("$?DIFF8|?DIFF23[][n]", 100, &c, 23), "");
        assert_eq!(journal("$?!diff16[n][m]", 100, &c, 14), "n");
        // A condition on anything but a difficulty holds, as a tooltip's
        // first branch; an else-chain's bare branch is the fallback.
        assert_eq!(journal("$?s137046[a][b]", 100, &c, 14), "a");
        assert_eq!(journal("$?diff1[a]?diff2[b][c]", 100, &c, 2), "b");
        assert_eq!(journal("$?diff1[a]?diff2[b][c]", 100, &c, 8), "c");
        // A tooltip never reads the journal's markup.
        assert_eq!(
            substitute("$[!16 x$] |Hspell:1|h[y]|h", 100, &c, 0),
            "$[!16 x$] |Hspell:1|h[y]|h"
        );
        assert_eq!(substitute("$?diff16[a][b]", 100, &c, 0), "a");
    }

    #[test]
    fn spell_refs_are_where_a_spell_must_stand() {
        assert_eq!(
            spell_refs(
                "within $1284506A1 yds, $s1 dmg, $@spellname77 and $@spelldesc88 \
                 ${$12s1*2} $?s137001[x][y] $@spellicon5 |Hspell:99|h[z]|h $$5 $"
            ),
            [
                (1284506, false),
                (77, false),
                (88, true),
                (12, false),
                (99, false)
            ]
        );
    }

    #[test]
    fn recursion_is_bounded() {
        let mut c = ctx();
        c.vars.insert(300, parse_variables("$again=$again"));
        assert_eq!(substitute("$again", 300, &c, 0), "$again");
        assert_eq!(substitute("x", 300, &c, 9), "x");
    }

    #[test]
    fn variable_blocks_join_continuation_lines() {
        let v = parse_variables("$dmg=1\n  + 2\n$x=3\n\n$bad name=4\n$=5\n");
        assert_eq!(v.get("dmg").map(String::as_str), Some("1 + 2"));
        assert_eq!(v.get("x").map(String::as_str), Some("3  $bad name=4 $=5"));
        assert_eq!(v.len(), 2);
        assert!(parse_variables("no definitions here").is_empty());
    }

    #[test]
    fn power_names_and_scales() {
        let names: Vec<&str> = (0..20).map(power_name).collect();
        assert_eq!(
            names,
            [
                "mana",
                "rage",
                "focus",
                "energy",
                "combo points",
                "runes",
                "runic power",
                "soul shards",
                "astral power",
                "holy power",
                "power",
                "maelstrom",
                "chi",
                "insanity",
                "power",
                "power",
                "arcane charges",
                "fury",
                "pain",
                "essence",
            ]
        );
        assert_eq!(power_name(-1), "power");
        assert_eq!(power_scale(1), 10.0);
        assert_eq!(power_scale(7), 10.0);
        assert_eq!(power_scale(13), 100.0);
        assert_eq!(power_scale(0), 1.0);
    }

    // ---- the whole pipeline over synthetic tables ----

    fn csv(text: &str) -> Csv {
        crate::table::parse_csv(text).unwrap()
    }

    /// Spell tables for five wanted spells. `points_col` picks the
    /// SpellEffect base-points spelling (both are seen in the wild).
    fn tables(points_col: &str) -> Tables {
        let mut t = Tables::default();
        t.insert(
            "Spell",
            csv("ID,Description_lang,AuraDescription_lang\n\
                 100,\"Deals $s1 Fire damage over $d to enemies within $a1 yds. \
                 $?s200[Empowered by $@spellname200.][Plain.]\",\n\
                 200,,\"Increases damage by $s1% for $d, matching $800s1.\"\n\
                 300,\"Hits for $s1, then $<bonus> more, $two.\",\n\
                 400,,\n\
                 600,\"A\r\nB $12345s1 C.\",\n\
                 800,Unwanted $s1.,\n\
                 x,junk,\n"),
        );
        t.insert(
            "SpellMisc",
            csv(
                "SpellID,DifficultyID,DurationIndex,RangeIndex,CastingTimeIndex\n\
                 100,1,2,1,1\n100,0,1,1,1\n200,0,2,0,0\n500,0,0,2,2\n\
                 800,0,1,0,0\n999,0,1,1,1\nx,0,0,0,0\n",
            ),
        );
        t.insert(
            "SpellRadius",
            csv("ID,Radius,RadiusMax\n1,8,0\n2,0,40\nx,1,1\n"),
        );
        t.insert(
            "SpellEffect",
            csv(&format!(
                "SpellID,EffectIndex,EffectAuraPeriod,{points_col},\
                 EffectRadiusIndex_0,EffectRadiusIndex_1,DifficultyID\n\
                 100,1,2000,12,0,0,0\n100,0,0,-50,1,0,0\n100,0,0,999,0,0,1\n\
                 200,0,0,30,0,2,0\n300,0,0,7,0,0,0\n800,0,0,5,0,0,\n\
                 999,0,0,1,0,0,0\nx,0,0,0,0,0,0\n"
            )),
        );
        t.insert("SpellDuration", csv("ID,Duration\n1,12000\n2,-1\nx,y\n"));
        t.insert(
            "SpellRange",
            csv("ID,RangeMax_0,RangeMax_1\n0,0,0\n1,40,0\n2,0,30\nx,0,0\n"),
        );
        t.insert("SpellCastTimes", csv("ID,Base\n0,0\n1,1500\n2,0\nx,y\n"));
        t.insert(
            "SpellPower",
            csv("SpellID,ManaCost,PowerCostPct,PowerType\n\
                 500,0,5,0\n500,100,0,0\n100,0,0,0\n100,300,0,7\n\
                 300,2500,0,13\n200,50,0,1\n999,1,0,0\nx,1,0,0\n"),
        );
        t.insert(
            "SpellDescriptionVariables",
            csv("ID,Variables\n1,\"$bonus=${$s1*3}\n$two=$s1\n plus\"\nx,\n"),
        );
        t.insert(
            "SpellXDescriptionVariables",
            csv("SpellID,SpellDescriptionVariablesID\n300,1\n100,99\n999,1\nx,1\n"),
        );
        t.insert(
            "SpellAuraOptions",
            csv(
                "SpellID,DifficultyID,ProcChance,CumulativeAura,ProcCategoryRecovery\n\
                 100,1,50,9,1\n100,0,15,3,8000\n100,0,99,9,9\n300,,20,0,0\n\
                 999,0,1,1,1\nx,0,1,1,1\n",
            ),
        );
        t
    }

    fn run(points_col: &str) -> HashMap<u32, Tip> {
        let wanted: HashSet<u32> = [100, 200, 300, 400, 500, 600].into();
        let names: HashMap<u32, String> = [(200, "Corruption".to_string())].into();
        let ranks: HashMap<u32, u32> = [(100, 2), (300, 1)].into();
        collect(&tables(points_col), &wanted, &names, &ranks).unwrap()
    }

    #[test]
    fn collect_renders_every_tooltip_line() {
        let out = run("EffectBasePointsF");
        let mut ids: Vec<u32> = out.keys().copied().collect();
        ids.sort_unstable();
        // 400 has no text, cost or range; 800 was never wanted.
        assert_eq!(ids, [100, 200, 300, 500, 600]);

        let t = &out[&100];
        assert_eq!(
            t.desc,
            "Deals 50 Fire damage over 12 sec to enemies within 8 yds. \
             Empowered by Corruption."
        );
        assert_eq!(
            t.desc_ranks,
            [
                "Deals 50 Fire damage over 12 sec to enemies within 8 yds. \
                 Empowered by Corruption.",
                "Deals 100 Fire damage over 12 sec to enemies within 8 yds. \
                 Empowered by Corruption."
            ]
        );
        assert_eq!(t.cost, "30 soul shards");
        assert_eq!(t.range, "40 yd range");
        assert_eq!(t.cast, "1.5 sec cast");

        // Aura text fills in for a pure passive; the cross-spell reference
        // to unwanted spell 800 resolves through the second-level harvest;
        // radius index 2 falls back to RadiusMax.
        let t = &out[&200];
        assert_eq!(
            t.desc,
            "Increases damage by 30% for until cancelled, matching 5."
        );
        assert!(t.desc_ranks.is_empty());
        assert_eq!(t.cost, "5 rage");
        assert_eq!(t.range, "");
        assert_eq!(t.cast, "Instant");

        // Description variables, single-rank (no rank variants).
        let t = &out[&300];
        assert_eq!(t.desc, "Hits for 7, then 21 more, 7 plus.");
        assert!(t.desc_ranks.is_empty());
        assert_eq!(t.cost, "25 insanity");
        assert_eq!((t.range.as_str(), t.cast.as_str()), ("", ""));

        // Cost/range/cast without any description text.
        let t = &out[&500];
        assert_eq!(t.desc, "");
        assert_eq!(t.cost, "5% of base mana");
        assert_eq!(t.range, "30 yd range");
        assert_eq!(t.cast, "Instant");

        // Line endings normalize and a dead reference's gap collapses.
        assert_eq!(out[&600].desc, "A\nB C.");
    }

    #[test]
    fn collect_accepts_the_older_points_column() {
        let out = run("EffectBasePoints");
        assert_eq!(out[&300].desc, "Hits for 7, then 21 more, 7 plus.");
    }

    #[test]
    fn collect_reports_missing_tables_and_columns() {
        let wanted: HashSet<u32> = [100].into();
        let names = HashMap::new();
        let ranks = HashMap::new();
        let mut t = tables("EffectBasePointsF");
        t.remove("SpellAuraOptions");
        assert_eq!(
            collect(&t, &wanted, &names, &ranks).unwrap_err(),
            "missing table SpellAuraOptions"
        );
        t.insert("SpellAuraOptions", csv("SpellID\n1\n"));
        assert!(
            collect(&t, &wanted, &names, &ranks)
                .unwrap_err()
                .contains("no column \"ProcChance\"")
        );
        let mut t = tables("EffectBasePointsF");
        t.insert("SpellEffect", csv("SpellID,EffectIndex,EffectAuraPeriod\n"));
        assert!(
            collect(&t, &wanted, &names, &ranks)
                .unwrap_err()
                .contains("EffectBasePoints")
        );
    }
}
