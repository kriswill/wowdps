//! Accent folding for the row filter: "akanos" has to find `Akanôs`.
//!
//! **What it covers.** Lowercasing, then diacritic stripping over Latin-1
//! Supplement (U+00C0–U+00FF) and Latin Extended-A (U+0100–U+017F) — the
//! range WoW's EU/US realms actually permit in a character name. That
//! includes the multi-character expansions no accent-stripping would catch
//! (ß→ss, æ→ae, œ→oe, þ→th) and the struck-through letters where the mark is
//! part of the glyph rather than a combining accent (ø→o, đ→d, ł→l, ħ→h).
//!
//! **What it deliberately does not.** Non-Latin scripts pass through
//! untouched: a Cyrillic name does not answer to Latin letters, and that is
//! the honest outcome, not a gap. Transliterating Кто into "kto" would mean
//! guessing a romanisation the player never chose and inventing matches
//! nobody typed — do not "improve" this into a transliteration table.
//! Combining marks (U+0300–U+036F) are also left alone: `std` has no Unicode
//! normalisation, so a name written as `o` + combining circumflex is a
//! different string from `ô` and only the latter folds.
//!
//! **Cost.** The filter runs at 10 Hz over up to 40 rows, so nothing here
//! allocates: the needle is folded once per keystroke into a `Vec<char>`
//! ([`fold`]), and each row is compared against it through a folding
//! ITERATOR over the row's own text ([`contains`]).

/// One folded character: most fold to themselves or to a single ASCII
/// letter, a handful expand to two.
#[derive(Debug, Clone, Copy)]
enum Fold {
    One(char),
    Two(char, char),
}

/// Lowercase `c`, then strip its diacritic. Unmapped characters — every
/// script this table does not cover — come back lowercased and otherwise
/// untouched.
fn fold_char(c: char) -> Fold {
    // `to_lowercase` can yield several chars (İ → i + U+0307); the first is
    // the letter and the rest are marks this fold has nothing to say about.
    let c = if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    };
    match c {
        // ---- Latin-1 Supplement ------------------------------------------
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => Fold::One('a'),
        'æ' => Fold::Two('a', 'e'),
        'ç' => Fold::One('c'),
        'è' | 'é' | 'ê' | 'ë' => Fold::One('e'),
        'ì' | 'í' | 'î' | 'ï' => Fold::One('i'),
        'ð' => Fold::One('d'),
        'ñ' => Fold::One('n'),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => Fold::One('o'),
        'ù' | 'ú' | 'û' | 'ü' => Fold::One('u'),
        'ý' | 'ÿ' => Fold::One('y'),
        'þ' => Fold::Two('t', 'h'),
        'ß' => Fold::Two('s', 's'),
        // ---- Latin Extended-A --------------------------------------------
        'ā' | 'ă' | 'ą' => Fold::One('a'),
        'ć' | 'ĉ' | 'ċ' | 'č' => Fold::One('c'),
        'ď' | 'đ' => Fold::One('d'),
        'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => Fold::One('e'),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => Fold::One('g'),
        'ĥ' | 'ħ' => Fold::One('h'),
        'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => Fold::One('i'),
        'ĳ' => Fold::Two('i', 'j'),
        'ĵ' => Fold::One('j'),
        'ķ' | 'ĸ' => Fold::One('k'),
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => Fold::One('l'),
        'ń' | 'ņ' | 'ň' | 'ŋ' => Fold::One('n'),
        'ŉ' => Fold::Two('n', 'n'),
        'ō' | 'ŏ' | 'ő' => Fold::One('o'),
        'œ' => Fold::Two('o', 'e'),
        'ŕ' | 'ŗ' | 'ř' => Fold::One('r'),
        'ś' | 'ŝ' | 'ş' | 'š' | 'ſ' => Fold::One('s'),
        'ţ' | 'ť' | 'ŧ' => Fold::One('t'),
        'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => Fold::One('u'),
        'ŵ' => Fold::One('w'),
        'ŷ' => Fold::One('y'),
        'ź' | 'ż' | 'ž' => Fold::One('z'),
        // Every other script, unchanged.
        other => Fold::One(other),
    }
}

/// A folding view over a string's characters. `Clone` because substring
/// search restarts the haystack at each candidate position, and cloning an
/// iterator is what lets it do that without allocating.
#[derive(Clone)]
struct Folded<'a> {
    inner: std::str::Chars<'a>,
    /// The second half of an expansion, waiting to be yielded.
    pending: Option<char>,
}

impl Iterator for Folded<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if let Some(c) = self.pending.take() {
            return Some(c);
        }
        match fold_char(self.inner.next()?) {
            Fold::One(c) => Some(c),
            Fold::Two(a, b) => {
                self.pending = Some(b);
                Some(a)
            }
        }
    }
}

fn folded(s: &str) -> Folded<'_> {
    Folded {
        inner: s.chars(),
        pending: None,
    }
}

/// Fold a needle, once. The caller does this per keystroke and hands the
/// result to [`contains`] for every row.
pub(crate) fn fold(needle: &str) -> Vec<char> {
    folded(needle).collect()
}

/// Does `haystack`, folded, contain the already-folded `needle`? An empty
/// needle contains trivially, matching `str::contains`.
pub(crate) fn contains(haystack: &str, needle: &[char]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut start = folded(haystack);
    loop {
        let mut here = start.clone();
        if needle.iter().all(|want| here.next() == Some(*want)) {
            return true;
        }
        // Advance one folded character and try again. Stepping the FOLDED
        // stream (not the raw one) is what lets a needle start inside an
        // expansion — "s" finds the second half of ß.
        if start.next().is_none() {
            return false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str) -> String {
        folded(s).collect()
    }

    #[test]
    fn plain_letters_find_accented_names() {
        // Both real rows from the user's store.
        assert!(contains("Akanôs-Tichondrius-US", &fold("akanos")));
        assert!(contains("Fidèle-Tichondrius-US", &fold("fidele")));
        // And the accented spelling still finds it: someone with a compose
        // key typing the name correctly must not be punished for it.
        assert!(contains("Akanôs-Tichondrius-US", &fold("akanôs")));
        assert!(contains("Akanôs-Tichondrius-US", &fold("AKANÔS")));
        assert!(contains("Fidèle-Tichondrius-US", &fold("Fidèle")));
        // A fragment through the accent, from either side.
        assert!(contains("Akanôs-Tichondrius-US", &fold("nos")));
        assert!(contains("Mírelle-Nebula-US", &fold("mirelle")));
        // And a genuine miss is still a miss.
        assert!(!contains("Akanôs-Tichondrius-US", &fold("akanas")));
    }

    #[test]
    fn the_multi_character_expansions() {
        assert_eq!(f("Straße"), "strasse");
        assert!(contains("Straße", &fold("strasse")));
        assert!(contains("Strasse", &fold("straße")));
        assert_eq!(f("Æther"), "aether");
        assert!(contains("Æther", &fold("aether")));
        assert_eq!(f("Œuvre"), "oeuvre");
        assert!(contains("Œuvre", &fold("oeuvre")));
        // A needle may start inside an expansion.
        assert!(contains("Straße", &fold("sse")));
    }

    #[test]
    fn the_letters_no_accent_stripping_catches() {
        assert_eq!(f("Sø ren"), "so ren");
        assert_eq!(f("Đurđa"), "durda");
        assert_eq!(f("Łukasz"), "lukasz");
        assert_eq!(f("Ħagar"), "hagar");
        assert_eq!(f("Þór"), "thor");
        assert!(contains("Sørine", &fold("sorine")));
        assert!(contains("Łucja", &fold("lucja")));
    }

    /// The deliberate limit. A Cyrillic name is left exactly as it is and
    /// answers to no Latin input — transliterating it would invent matches
    /// nobody typed.
    #[test]
    fn cyrillic_is_untouched_and_unmatched() {
        let name = "Дракон-Гордунни";
        assert_eq!(f(name), name.to_lowercase(), "no transliteration");
        assert!(!contains(name, &fold("drakon")));
        assert!(!contains(name, &fold("d")));
        // It still answers to itself, case-folded.
        assert!(contains(name, &fold("дракон")));
        assert!(contains(name, &fold("ДРАКОН")));
        // And Latin rows are not matched by Cyrillic input.
        assert!(!contains("Dragon-Nebula-US", &fold("дракон")));
    }

    #[test]
    fn an_empty_needle_matches_anything() {
        assert!(contains("whatever", &fold("")));
        assert!(contains("", &fold("")));
        assert!(!contains("", &fold("a")));
    }

    #[test]
    fn ascii_is_unchanged_apart_from_case() {
        assert_eq!(f("Thraxx-Nebula-US"), "thraxx-nebula-us");
        assert!(contains("Thraxx-Nebula-US", &fold("THRAXX")));
        assert!(!contains("Thraxx-Nebula-US", &fold("zz")));
    }
}
