//! R29: a title's pieces, what names a boss. Its own file so the
//! real-log gate (`tests/real_log_boss_names.rs`) reads titles with this
//! same code (`#[path]`), with no public API for it.

/// R29: a title's parts — the whole, then each name a comma or a lone
/// "and" divides it into, lowercased (ASCII only) — what names a boss.
pub(crate) fn title_parts(title: &str) -> Vec<String> {
    let b = title.as_bytes();
    let at = |i: usize| b.get(i).copied();
    let word = |i: usize| at(i).is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
    let space = |i: usize| at(i).is_some_and(|c| c.is_ascii_whitespace());
    let mut parts = vec![title.to_ascii_lowercase()];
    // The separators: a comma, or "and" standing alone, with the white
    // space round them.
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let sep = if at(i) == Some(b',') {
            Some(i + 1)
        } else if b.get(i..).is_some_and(|r| r.starts_with(b"and"))
            && (i == 0 || !word(i - 1))
            && !word(i + 3)
        {
            Some(i + 3)
        } else {
            None
        };
        match sep {
            Some(end) => {
                let mut s = i;
                while s > 0 && space(s - 1) {
                    s -= 1;
                }
                let mut e = end;
                while space(e) {
                    e += 1;
                }
                cuts.push((s, e));
                i = end;
            }
            None => i += 1,
        }
    }
    let mut pieces = Vec::new();
    let mut from = 0;
    for (s, e) in cuts {
        // A cut inside white space already taken by the last one.
        let s = s.max(from);
        pieces.push(title.get(from..s).unwrap_or(""));
        from = e.max(s);
    }
    pieces.push(title.get(from..).unwrap_or(""));
    while pieces.last() == Some(&"") {
        pieces.pop();
    }
    parts.extend(pieces.iter().map(|p| p.to_ascii_lowercase()));
    parts
}
