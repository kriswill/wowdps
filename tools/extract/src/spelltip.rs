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
//! else is left in place rather than guessed at.

use std::collections::{HashMap, HashSet};

use crate::table::Csv;

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
type Effect = (u32, f64, u32, f64);

struct Ctx {
    /// spell → effects, ascending by index.
    effects: HashMap<u32, Vec<Effect>>,
    /// spell → duration ms (-1 = until cancelled).
    durations: HashMap<u32, i64>,
    names: HashMap<u32, String>,
    /// Raw descriptions of cross-referenced spells, for `$@spelldesc<id>`.
    descs: HashMap<u32, String>,
    /// spell → its description variables (`$dmg=…` definitions), used by
    /// `$<dmg>` and bare `$dmg` references.
    vars: HashMap<u32, HashMap<String, String>>,
    /// spell → (proc chance %, max stacks, proc icd ms) from
    /// SpellAuraOptions, for `$h`, `$u` and `$proccooldown`.
    aura: HashMap<u32, (f64, u32, u32)>,
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

fn cell(row: &[String], c: usize) -> &str {
    row.get(c).map(String::as_str).unwrap_or("")
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

/// Evaluate `a op b op c` with * and / before + and -; None on anything
/// that is not plain arithmetic over numbers.
fn eval_arith(s: &str) -> Option<f64> {
    let mut terms: Vec<f64> = Vec::new();
    let mut term: Option<f64> = None;
    let mut pending_mul: Option<char> = None;
    let mut pending_add: Option<char> = None;
    for tok in tokenize_arith(s)? {
        match tok {
            ArithTok::Num(v) => {
                let v = match pending_mul.take() {
                    Some('*') => term.take()? * v,
                    Some('/') => {
                        let t = term.take()?;
                        if v == 0.0 {
                            return None;
                        }
                        t / v
                    }
                    _ => v,
                };
                term = Some(v);
            }
            ArithTok::Op(op @ ('*' | '/')) => {
                pending_mul = Some(op);
            }
            ArithTok::Op(op @ ('+' | '-')) => {
                let t = term.take()?;
                terms.push(if pending_add == Some('-') { -t } else { t });
                pending_add = Some(op);
            }
            ArithTok::Op(_) => return None,
        }
    }
    let t = term?;
    terms.push(if pending_add == Some('-') { -t } else { t });
    Some(terms.iter().sum())
}

enum ArithTok {
    Num(f64),
    Op(char),
}

fn tokenize_arith(s: &str) -> Option<Vec<ArithTok>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '0'..='9' | '.' => cur.push(c),
            '+' | '-' | '*' | '/' => {
                if !cur.is_empty() {
                    out.push(ArithTok::Num(cur.parse().ok()?));
                    cur.clear();
                }
                out.push(ArithTok::Op(c));
            }
            ' ' => {
                if !cur.is_empty() {
                    out.push(ArithTok::Num(cur.parse().ok()?));
                    cur.clear();
                }
            }
            _ => return None,
        }
    }
    if !cur.is_empty() {
        out.push(ArithTok::Num(cur.parse().ok()?));
    }
    (!out.is_empty()).then_some(out)
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
    let vals: Vec<f64> = parts
        .iter()
        .map(|p| eval_arith(p.trim()))
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
            let substituted = substitute_scaled(&inner, spell, ctx, depth + 1, mult);
            match eval_arith(&substituted) {
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
        // $?cond[a]…[b] — take the first branch, drop the rest.
        if chars.get(i) == Some(&'?') {
            let mut j = i + 1;
            while j < chars.len() && chars.get(j) != Some(&'[') {
                j += 1;
            }
            let Some((first, mut k)) = bracket(&chars, j) else {
                out.push('$');
                continue;
            };
            // Consume else-chains: ?cond[x] and bare [x].
            loop {
                if chars.get(k) == Some(&'?') {
                    let mut m = k + 1;
                    while m < chars.len() && chars.get(m) != Some(&'[') {
                        m += 1;
                    }
                    match bracket(&chars, m) {
                        Some((_, next)) => k = next,
                        None => break,
                    }
                } else if chars.get(k) == Some(&'[') {
                    match bracket(&chars, k) {
                        Some((_, next)) => k = next,
                        None => break,
                    }
                } else {
                    break;
                }
            }
            out.push_str(&substitute_scaled(&first, spell, ctx, depth + 1, mult));
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
                    out.push_str(substitute(text, id, ctx, depth + 1).trim());
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
                out.push_str(substitute_scaled(&expr.clone(), spell, ctx, depth + 1, mult).trim());
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
            out.push_str(substitute_scaled(&expr.clone(), spell, ctx, depth + 1, mult).trim());
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
                let substituted = substitute_scaled(&inner, spell, ctx, depth + 1, mult);
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
fn col2(csv: &Csv, a: &str, b: &str) -> Result<usize, String> {
    csv.col(a).or_else(|_| csv.col(b))
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
fn ids_in(text: &str, out: &mut HashSet<u32>) {
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

/// Build tooltip lines for every wanted spell. `ranks` names each spell's
/// talent max-rank; spells ranked above 1 also get per-rank descriptions.
pub fn collect(
    tables: &HashMap<&str, Csv>,
    wanted: &HashSet<u32>,
    names: &HashMap<u32, String>,
    ranks: &HashMap<u32, u32>,
) -> Result<HashMap<u32, Tip>, String> {
    let get = |name: &str| -> Result<&Csv, String> {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };

    // Raw descriptions for the wanted spells; Description wins, the aura
    // text fills in for pure passives.
    let spell = get("Spell")?;
    let (s_id, s_desc, s_aura) = (
        spell.col("ID")?,
        spell.col("Description_lang")?,
        spell.col("AuraDescription_lang")?,
    );
    // Every spell's text stays available: `$@spelldesc<id>` embeds other
    // spells' descriptions, and those can reference further spells.
    let mut all_descs: HashMap<u32, String> = HashMap::new();
    for r in &spell.rows {
        let Ok(id) = cell(r, s_id).parse::<u32>() else {
            continue;
        };
        let desc = cell(r, s_desc);
        let text = if desc.is_empty() {
            cell(r, s_aura)
        } else {
            desc
        };
        if !text.is_empty() {
            all_descs.insert(id, text.to_string());
        }
    }
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
    let sm = get("SpellMisc")?;
    let (m_spell, m_dur, m_range, m_cast) = (
        sm.col("SpellID")?,
        sm.col("DurationIndex")?,
        sm.col("RangeIndex")?,
        sm.col("CastingTimeIndex")?,
    );
    let m_diff = sm.col("DifficultyID").ok();
    let mut misc: HashMap<u32, (u32, u32, u32)> = HashMap::new();
    for r in &sm.rows {
        let Ok(id) = cell(r, m_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) {
            continue;
        }
        let base = m_diff.map(|c| cell(r, c)).is_none_or(|d| d == "0");
        if base || !misc.contains_key(&id) {
            misc.insert(
                id,
                (
                    num(cell(r, m_dur)) as u32,
                    num(cell(r, m_range)) as u32,
                    num(cell(r, m_cast)) as u32,
                ),
            );
        }
    }

    // SpellRadius: index → yards (RadiusMax fills in for min/max pairs).
    let sradius = get("SpellRadius")?;
    let (ra_id, ra_r, ra_max) = (
        sradius.col("ID")?,
        sradius.col("Radius")?,
        sradius.col("RadiusMax")?,
    );
    let radius_yd: HashMap<u32, f64> = sradius
        .rows
        .iter()
        .filter_map(|r| {
            let v = num(cell(r, ra_r));
            let v = if v > 0.0 { v } else { num(cell(r, ra_max)) };
            Some((cell(r, ra_id).parse().ok()?, v))
        })
        .collect();

    // SpellEffect: base points, aura periods and radii by effect index.
    let se = get("SpellEffect")?;
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
        let Ok(id) = cell(r, e_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) {
            continue;
        }
        if e_diff
            .map(|c| cell(r, c))
            .is_some_and(|d| !d.is_empty() && d != "0")
        {
            continue;
        }
        let rad_ix = match num(cell(r, e_rad0)) as u32 {
            0 => num(cell(r, e_rad1)) as u32,
            ix => ix,
        };
        effects.entry(id).or_default().push((
            num(cell(r, e_index)) as u32,
            num(cell(r, e_points)),
            num(cell(r, e_period)) as u32,
            radius_yd.get(&rad_ix).copied().unwrap_or(0.0),
        ));
    }
    for list in effects.values_mut() {
        list.sort_unstable_by_key(|(i, _, _, _)| *i);
    }

    // Duration / range / cast lookup tables.
    let sd = get("SpellDuration")?;
    let (d_id, d_ms) = (sd.col("ID")?, sd.col("Duration")?);
    let dur_by_index: HashMap<u32, i64> = sd
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                cell(r, d_id).parse().ok()?,
                cell(r, d_ms).parse::<i64>().ok()?,
            ))
        })
        .collect();

    let sr = get("SpellRange")?;
    let (r_id, r_max0, r_max1) = (sr.col("ID")?, sr.col("RangeMax_0")?, sr.col("RangeMax_1")?);
    let range_by_index: HashMap<u32, f64> = sr
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                cell(r, r_id).parse().ok()?,
                num(cell(r, r_max0)).max(num(cell(r, r_max1))),
            ))
        })
        .collect();

    let sc = get("SpellCastTimes")?;
    let (c_id, c_base) = (sc.col("ID")?, sc.col("Base")?);
    let cast_by_index: HashMap<u32, i64> = sc
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                cell(r, c_id).parse().ok()?,
                cell(r, c_base).parse::<i64>().ok()?,
            ))
        })
        .collect();

    // SpellPower: the first nonzero cost line.
    let sp = get("SpellPower")?;
    let (p_spell, p_mana, p_pct, p_type) = (
        sp.col("SpellID")?,
        sp.col("ManaCost")?,
        sp.col("PowerCostPct")?,
        sp.col("PowerType")?,
    );
    let mut costs: HashMap<u32, String> = HashMap::new();
    for r in &sp.rows {
        let Ok(id) = cell(r, p_spell).parse::<u32>() else {
            continue;
        };
        if !wanted.contains(&id) || costs.contains_key(&id) {
            continue;
        }
        let power = power_name(num(cell(r, p_type)) as i64);
        let pct = num(cell(r, p_pct));
        let flat = num(cell(r, p_mana));
        let line = if pct > 0.0 {
            format!("{}% of base {power}", fmt_value(pct))
        } else if flat > 0.0 {
            // Whole-unit resources store ×10 (shards) or ×100 (insanity).
            format!(
                "{} {power}",
                fmt_value(flat / power_scale(num(cell(r, p_type)) as i64))
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

    // Description variables: `$dmg=…` definitions attached per spell,
    // referenced from the text as `$<dmg>` or bare `$dmg`.
    let sdv = get("SpellDescriptionVariables")?;
    let (v_id, v_text) = (sdv.col("ID")?, sdv.col("Variables")?);
    let var_sets: HashMap<u32, &str> = sdv
        .rows
        .iter()
        .filter_map(|r| Some((cell(r, v_id).parse().ok()?, cell(r, v_text))))
        .collect();
    let sxdv = get("SpellXDescriptionVariables")?;
    let (x_spell, x_vars) = (
        sxdv.col("SpellID")?,
        sxdv.col("SpellDescriptionVariablesID")?,
    );
    let mut vars: HashMap<u32, HashMap<String, String>> = HashMap::new();
    for r in &sxdv.rows {
        let Ok(id) = cell(r, x_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) {
            continue;
        }
        let Some(text) = cell(r, x_vars)
            .parse::<u32>()
            .ok()
            .and_then(|v| var_sets.get(&v))
        else {
            continue;
        };
        vars.insert(id, parse_variables(text));
    }

    // SpellAuraOptions: proc chance, max stacks, proc cooldown.
    let sao = get("SpellAuraOptions")?;
    let (o_spell, o_chance, o_stacks, o_icd) = (
        sao.col("SpellID")?,
        sao.col("ProcChance")?,
        sao.col("CumulativeAura")?,
        sao.col("ProcCategoryRecovery")?,
    );
    let o_diff = sao.col("DifficultyID").ok();
    let mut aura: HashMap<u32, (f64, u32, u32)> = HashMap::new();
    for r in &sao.rows {
        let Ok(id) = cell(r, o_spell).parse::<u32>() else {
            continue;
        };
        if !needed.contains(&id) || aura.contains_key(&id) {
            continue;
        }
        if o_diff
            .map(|c| cell(r, c))
            .is_some_and(|d| !d.is_empty() && d != "0")
        {
            continue;
        }
        aura.insert(
            id,
            (
                num(cell(r, o_chance)),
                num(cell(r, o_stacks)) as u32,
                num(cell(r, o_icd)) as u32,
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
        let polish = |s: String| -> String {
            let mut s = s.replace("\r\n", "\n").replace('\r', "\n");
            // Dropped dead-reference tokens leave doubled spaces behind.
            while s.contains("  ") {
                s = s.replace("  ", " ");
            }
            s
        };
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
        assert_eq!(eval_arith("50/10"), Some(5.0));
        assert_eq!(eval_arith("2+3*4"), Some(14.0));
        assert_eq!(eval_arith("50%"), None);
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
}
