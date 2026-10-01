//! The command palette's model (the prototype's `.pal`), free of any GUI:
//! what it lists — the pulls on the rail (the newest few before anything
//! is typed), the players of the pull on the stage, the views and the
//! window's screens, grouped — how a query narrows it (the row filter's
//! accent-folded substring, so "akanos" finds Akanôs), and the selection
//! the keys move. Moved from the iced window's palette so both window GUIs
//! list and rank the same things.

use wowdps_model::{Action, Class, Row, Spec, View};

use crate::fold;
use crate::home::{ALL_CHARACTERS, CharLine};
use crate::labels::{WINDOW_VIEWS, shown_name, window_view_name};
use crate::rail::{KeyWord, Mark, Pull, Rail};

#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
#[cfg(test)]
mod tests;

/// Pulls offered before anything is typed: the rail's newest, a live one
/// among them — the pulls a reader most often jumps back to.
pub const RECENT: usize = 4;
/// Pulls a query lists at most: the rail can hold hundreds, and a few more
/// letters narrow them.
pub const MAX_PULLS: usize = 20;

/// The palette's state: what is typed, and the selection the keys move —
/// the one line lit, and what Enter runs. The pointer lights nothing (the
/// prototype's `.pal-it` has no hover): a second wash under a resting
/// pointer would leave the reader asking which of two lines Enter meant.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Palette {
    pub query: String,
    /// Index into [`listed`]'s answer for the query.
    pub sel: usize,
}

impl Palette {
    /// A new query: the selection goes back to the top of what it lists.
    pub fn typed(&mut self, query: String) {
        self.query = query;
        self.sel = 0;
    }

    /// One step down (`down`) or up a list `len` long, stopping at its ends.
    pub fn step(&mut self, down: bool, len: usize) {
        self.clamp(len);
        self.sel = if down {
            (self.sel + 1).min(len.saturating_sub(1))
        } else {
            self.sel.saturating_sub(1)
        };
    }

    /// The list changed under the card (a live pull arrived, the players
    /// refilled after a view's answer, a page of the rail landed): the
    /// selection stays on a line it lists — the last, when it shrank past
    /// it — so Enter always runs something drawn (the prototype's `S.pi =
    /// min(S.pi, len - 1)` on every render).
    pub fn clamp(&mut self, len: usize) {
        self.sel = self.sel.min(len.saturating_sub(1));
    }
}

/// The groups, in the order the card lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Pulls,
    Players,
    Views,
    Screens,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Pulls => "Pulls",
            Group::Players => "Players in this pull",
            Group::Views => "Views",
            Group::Screens => "Screens",
        }
    }
}

/// What running an item does.
#[derive(Debug, Clone, PartialEq)]
pub enum Run {
    /// Put the pull on the stage.
    Pull(Pull),
    /// Select the player on the pull's meter (by key, as a drill names
    /// them), on a view they have a row on.
    Player {
        key: String,
        label: String,
    },
    View(View),
    /// Home (`~`), the live pull (`m`), the earlier nights on the rail
    /// (`H`), the `?` sheet, the talent viewer (`t`).
    Home,
    Live,
    Earlier,
    Sheet,
    Talents,
    /// Home, scoped to one character's guid (`None`: all of them) — what
    /// its chips do, for a reader on the keys.
    HomeScope(Option<String>),
}

/// One line of the card.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub group: Group,
    pub title: String,
    /// The quieter words at its right: a pull's night and verdict, a
    /// player's spec. Searched with the title.
    pub sub: String,
    /// The key that does the same, on a keycap at its right.
    pub key: Option<&'static str>,
    /// A player's disc: their spec icon, else their class's.
    pub disc: Option<(Option<Class>, Option<Spec>)>,
    /// A pull the empty query offers ([`RECENT`]).
    pub recent: bool,
    pub run: Run,
}

/// Every item the palette could list: the rail's pulls, the players of the
/// pull on the stage, the views and the screens — Home's scopes among them,
/// all characters and each of `chars` (the characters the window knows you
/// play). `players` are that pull's rows — our side's, whatever view they
/// were read on.
pub fn items(rail: &Rail, players: &[Row], chars: &[CharLine], hide_realms: bool) -> Vec<Item> {
    let shown = |name: &str| shown_name(name, hide_realms);
    let mut out = Vec::new();
    let mut recent = 0;
    for night in &rail.nights {
        for visit in &night.visits {
            for line in &visit.lines {
                // What the rail itself says at the row's right, as words.
                let verdict = match (line.mark, line.key, line.best_pct) {
                    (Mark::Live, ..) => Some("live".to_string()),
                    (Mark::Sum, ..) => Some("whole visit".to_string()),
                    (Mark::Good, Some(KeyWord::Plus(n)), _) => Some(format!("+{n}")),
                    (Mark::Bad, Some(KeyWord::Over), _) => Some("over".to_string()),
                    (Mark::Good, ..) if visit.title == "Arena" => Some("win".to_string()),
                    (Mark::Bad, ..) if visit.title == "Arena" => Some("loss".to_string()),
                    (Mark::Good, ..) => Some("kill".to_string()),
                    (Mark::Bad, _, Some(pct)) => Some(format!("wipe at {pct}%")),
                    (Mark::Bad, ..) => Some("wipe".to_string()),
                    (Mark::Dash, ..) => None,
                };
                let sub = match verdict {
                    Some(v) => format!("{}, {v}", night.label),
                    None => night.label.clone(),
                };
                // The newest few worth jumping back to: a live pull, a
                // boss, a key — not the trash between them, nor a visit's Σ.
                let offered = line.mark == Mark::Live || (!line.trash && line.mark != Mark::Sum);
                let is_recent = offered && recent < RECENT;
                recent += usize::from(is_recent);
                out.push(Item {
                    group: Group::Pulls,
                    // A visit's Σ goes by its visit: "The Venomous Abyss,
                    // Heroic", whole.
                    title: if line.mark == Mark::Sum {
                        visit.title.clone()
                    } else {
                        line.name.clone()
                    },
                    sub,
                    key: None,
                    disc: None,
                    recent: is_recent,
                    run: Run::Pull(line.pull.clone()),
                });
            }
        }
    }
    for r in players.iter().filter(|r| !r.enemy) {
        let sub = match (r.spec, r.class) {
            (Some(s), Some(c)) => format!("{} {}", s.name(), c.name()),
            (None, Some(c)) => c.name().to_string(),
            _ => String::new(),
        };
        out.push(Item {
            group: Group::Players,
            title: shown(&r.label),
            sub,
            key: None,
            disc: Some((r.class, r.spec)),
            recent: false,
            run: Run::Player {
                key: r.key.clone(),
                label: r.label.clone(),
            },
        });
    }
    for v in WINDOW_VIEWS {
        out.push(Item {
            group: Group::Views,
            title: window_view_name(v).to_string(),
            sub: String::new(),
            key: crate::keys::key_for(Action::SetView(v)),
            disc: None,
            recent: false,
            run: Run::View(v),
        });
    }
    for (title, key, run) in [
        ("Home", "~", Run::Home),
        ("Live pull", "m", Run::Live),
        ("Earlier nights", "H", Run::Earlier),
        ("Keyboard shortcuts", "?", Run::Sheet),
        ("Talents", "t", Run::Talents),
    ] {
        out.push(Item {
            group: Group::Screens,
            title: title.to_string(),
            sub: String::new(),
            key: Some(key),
            disc: None,
            recent: false,
            run,
        });
    }
    // Home's scope chips, for the keys: all characters, then each one.
    out.push(Item {
        group: Group::Screens,
        title: format!("Home: {}", ALL_CHARACTERS),
        sub: String::new(),
        key: None,
        disc: None,
        recent: false,
        run: Run::HomeScope(None),
    });
    for c in chars.iter().filter(|c| !c.guid.is_empty()) {
        out.push(Item {
            group: Group::Screens,
            title: format!("Home: {}", shown(&c.name)),
            sub: String::new(),
            key: None,
            disc: Some((c.class, c.spec)),
            recent: false,
            run: Run::HomeScope(Some(c.guid.clone())),
        });
    }
    out
}

/// What the card lists for `query`: before anything is typed, the recent
/// pulls and every other item but Home's scopes (the prototype's card,
/// which has none — they wait for a word, a name or "home"); then every
/// item whose title and words together contain it, accent-folded and case
/// aside — at most [`MAX_PULLS`] pulls.
pub fn listed(items: Vec<Item>, query: &str) -> Vec<Item> {
    let needle = fold::fold(query.trim());
    let mut pulls = 0;
    items
        .into_iter()
        .filter(|i| {
            if i.group == Group::Pulls {
                let keep = if needle.is_empty() {
                    i.recent
                } else {
                    pulls < MAX_PULLS && matches(i, &needle)
                };
                pulls += usize::from(keep);
                return keep;
            }
            if needle.is_empty() {
                return !matches!(i.run, Run::HomeScope(_));
            }
            matches(i, &needle)
        })
        .collect()
}

/// Does `item` answer to the folded `needle`? Its title and its words are
/// one text, a space between (the prototype's `fold(t + ' ' + s)`), so a
/// query may run from one into the other: "akanos devourer".
fn matches(item: &Item, needle: &[char]) -> bool {
    if item.sub.is_empty() {
        fold::contains(&item.title, needle)
    } else {
        fold::contains(&format!("{} {}", item.title, item.sub), needle)
    }
}
