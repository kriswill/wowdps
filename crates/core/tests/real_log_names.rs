//! Player labels over a real log, read by its edition
//! (`wowdps_model::names`): every label a `Player-` guid wears is a whole
//! player label (a name, its realm, its region) or a bare name, a Retail
//! name is one word and a Forever name two, and a lookup over the log's
//! players never names a player who is not the one asked for. A full name
//! names its own player; a first name that two players share names
//! neither, and one a player alone has names that player.
//!
//! A real log carries real names: nothing here prints a name or a guid, a
//! failure included. A failure says which rule broke and how many labels
//! or players broke it.
//!
//! Run: `WOWDPS_REAL_LOG=<Retail log> WOWDPS_REAL_LOG_FOREVER=<Forever
//! log> cargo test --release -p wowdps-core --test real_log_names --
//! --ignored --nocapture` (which log each reads: `tests/real_log/mod.rs`).
//! Both names hold `real`, and the Forever variant's `forever`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod real_log;

use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;

use wowdps_model::Edition;
use wowdps_model::names::{Found, Label, find};

/// The most players a lookup is asked about, each over the whole roster.
const ASKED: usize = 3_000;

/// Every label each `Player-` guid wears on the log's lines, as the meter
/// learns them: the quoted field after the guid.
fn labels(path: &str) -> BTreeMap<String, BTreeSet<String>> {
    let file = std::fs::File::open(path).expect("open the log");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in std::io::BufReader::new(file).split(b'\n') {
        let Ok(line) = String::from_utf8(line.expect("read a line")) else {
            continue;
        };
        let mut rest = line.as_str();
        while let Some(at) = rest.find("Player-") {
            rest = rest.get(at..).unwrap_or_default();
            let Some((guid, after)) = rest.split_once(',') else {
                break;
            };
            rest = after;
            let Some(quoted) = after.strip_prefix('"') else {
                continue;
            };
            let Some((name, _)) = quoted.split_once('"') else {
                continue;
            };
            if !name.is_empty() && name != "nil" {
                out.entry(guid.to_string())
                    .or_default()
                    .insert(name.to_string());
            }
        }
    }
    out
}

fn check(path: &str, edition: Edition) {
    let players = labels(path);
    let words = |l: &Label<'_>| l.name().split(' ').count();
    let want_words = if edition == Edition::Forever { 2 } else { 1 };
    let (mut whole, mut bare, mut odd, mut wrong_words) = (0, 0, 0, 0);
    for label in players.values().flatten() {
        if let Some(l) = Label::shaped(label, edition) {
            whole += 1;
            if words(&l) != want_words {
                wrong_words += 1;
            }
        } else if !label.contains('-') {
            bare += 1;
        } else {
            odd += 1;
        }
    }
    println!(
        "{edition:?}: {} players, {} labels: {whole} whole, {bare} bare, {odd} of neither shape; \
         {wrong_words} whole labels whose name is not {want_words} word(s)",
        players.len(),
        players.values().map(BTreeSet::len).sum::<usize>(),
    );
    assert_eq!(
        odd, 0,
        "{odd} labels are neither a whole player label nor a bare name"
    );
    assert_eq!(
        wrong_words, 0,
        "{wrong_words} names are not {want_words} word(s)"
    );

    // Each player by their whole label (a bare one where they wear no
    // other), as a lookup over the log reads them.
    let roster: Vec<(&str, &str)> = players
        .iter()
        .map(|(g, ls)| {
            let label = ls
                .iter()
                .find(|l| Label::shaped(l, edition).is_some())
                .or_else(|| ls.iter().next())
                .unwrap();
            (g.as_str(), label.as_str())
        })
        .collect();
    let mut by_first: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_name: BTreeMap<String, usize> = BTreeMap::new();
    for (_, label) in &roster {
        let l = Label::player(label, edition);
        *by_first.entry(l.first().to_lowercase()).or_default() += 1;
        *by_name.entry(l.name().to_lowercase()).or_default() += 1;
    }
    let shared_first = by_first.values().filter(|n| **n > 1).count();
    let shared_name = by_name.values().filter(|n| **n > 1).count();
    println!(
        "{edition:?}: {} first names, {shared_first} shared by two players or more; {} names, \
         {shared_name} shared",
        by_first.len(),
        by_name.len()
    );
    let all = || roster.iter().map(|&(g, l)| (g, l, edition));
    let (mut asked, mut wrong, mut refused_first, mut found_first) = (0, 0, 0, 0);
    for &(guid, label) in roster.iter().take(ASKED) {
        asked += 1;
        let l = Label::player(label, edition);
        // The label whole names its own player (with any other who wore
        // the very same label: a character made again under its name).
        match find(label, all()) {
            Found::One(g) if g == guid => {}
            Found::Several(gs) if gs.contains(&guid) => {}
            _ => wrong += 1,
        }
        // A name names its player, or several, never another alone.
        match find(&l.name(), all()) {
            Found::One(g) if g != guid => wrong += 1,
            Found::Several(gs) if !gs.contains(&guid) => wrong += 1,
            Found::Nobody => wrong += 1,
            _ => {}
        }
        if edition == Edition::Forever && l.realm().is_some() {
            let shared = by_first
                .get(&l.first().to_lowercase())
                .copied()
                .unwrap_or(0)
                > 1;
            match find(l.first(), all()) {
                Found::One(g) if g == guid && !shared => found_first += 1,
                Found::Several(gs) if shared && gs.contains(&guid) => refused_first += 1,
                _ => wrong += 1,
            }
        }
    }
    println!(
        "{edition:?}: {asked} players looked up: {wrong} wrong; by first name alone {found_first} \
         found, {refused_first} refused as shared"
    );
    assert_eq!(
        wrong, 0,
        "{wrong} lookups named a player other than the one asked for"
    );
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG pointing at a Retail combat log"]
fn player_labels_read_by_edition_on_a_real_log() {
    let Some(path) = real_log::retail_log() else {
        return;
    };
    check(&path, Edition::Retail);
}

#[test]
#[ignore = "needs WOWDPS_REAL_LOG_FOREVER pointing at a Forever combat log"]
fn player_labels_read_by_edition_on_a_real_forever_log() {
    let Some(path) = real_log::forever_log() else {
        return;
    };
    check(&path, Edition::Forever);
}
