//! Difficulties by name, and the order a difficulty's overrides apply in.
//!
//! A file names a difficulty by word (`[difficulty.mythic]`), never by id:
//! a dungeon's Mythic (23) and a raid's (16) are both "mythic". A difficulty
//! inherits what the ones it falls back to say, as the client's own
//! `Difficulty.FallbackDifficultyID` chain has it (`docs/replay-assets.md`
//! §3): a keystone reads Mythic's, then Heroic's, then Normal's, and its own
//! overrides last.

/// Every difficulty name a file may use.
pub const NAMES: &[&str] = &[
    "normal",
    "heroic",
    "mythic",
    "keystone",
    "lfr",
    "world",
    "story",
    "lorewalking",
    "timewalking",
    "follower",
];

/// Every difficulty, in `NAMES`' order: the ids the log writes for it, and
/// its chain, its own name then the names it falls back to, the most
/// specific first. What `chain` and `fallbacks` both read.
const CHAINS: [(&[u32], &[&str]); 10] = [
    // A dungeon's (1) and a raid's (14).
    (&[1, 14], &["normal"]),
    (&[2, 15], &["heroic", "normal"]),
    // A dungeon's (23), a raid's (16) and its flex form (233).
    (&[16, 23, 233], &["mythic", "heroic", "normal"]),
    // Mythic Keystone, which runs a dungeon's Mythic.
    (&[8], &["keystone", "mythic", "heroic", "normal"]),
    (&[17], &["lfr", "normal"]),
    (&[250], &["world", "normal"]),
    (&[220], &["story"]),
    (&[241], &["lorewalking"]),
    (&[24, 33], &["timewalking"]),
    (&[205], &["follower"]),
];

/// A difficulty id's name and the names it falls back to, the most
/// specific first. An unknown id has none.
pub fn chain(id: u32) -> &'static [&'static str] {
    CHAINS
        .iter()
        .find(|(ids, _)| ids.contains(&id))
        .map_or(&[], |(_, chain)| chain)
}

/// A difficulty name's chain: the name, then the names it falls back to,
/// the most specific first, as `chain` gives it for the name's ids. An
/// unknown name has none.
pub fn chain_named(name: &str) -> &'static [&'static str] {
    CHAINS
        .iter()
        .find(|(_, chain)| chain.first() == Some(&name))
        .map_or(&[], |(_, chain)| chain)
}

/// The names a difficulty name falls back to, the most specific first: its
/// chain less itself. An unknown name has none.
pub fn fallbacks(name: &str) -> &'static [&'static str] {
    chain_named(name).get(1..).unwrap_or(&[])
}

/// Whether an entry limited to `only` (difficulty names; empty: all)
/// appears on the difficulty whose chain this is: its own name, exactly,
/// not the ones it falls back to. The journal lists an ability's
/// difficulties exactly (a Heroic copy of a section beside a Mythic one;
/// `[8, 23]` for a dungeon's keystone and Mythic), and presence does not
/// inherit the way values do: what Heroic alone has, Mythic lacks.
pub fn applies(only: &[String], chain: &[&str]) -> bool {
    only.is_empty()
        || chain
            .first()
            .is_some_and(|own| only.iter().any(|o| o == own))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One row per name, in `NAMES`' order, each falling back only to
    /// names that fall back the same way: so a name's chain is the one
    /// every id of it reads, and an override laid the most general first
    /// finds every overlay it inherits already laid.
    #[test]
    fn every_name_heads_one_chain_of_names() {
        let heads: Vec<&str> = CHAINS.iter().map(|(_, c)| c[0]).collect();
        assert_eq!(heads, NAMES);
        for (ids, want) in CHAINS {
            for &id in ids {
                assert_eq!(chain(id), want, "{id}");
            }
            for (i, name) in want.iter().enumerate() {
                assert!(NAMES.contains(name), "{name}");
                assert_eq!(chain_named(name), &want[i..], "{name}");
            }
        }
        assert_eq!(fallbacks("keystone"), ["mythic", "heroic", "normal"]);
        assert!(chain(0).is_empty() && chain_named("mythc").is_empty());
        assert!(fallbacks("mythc").is_empty() && fallbacks("normal").is_empty());
    }
}
