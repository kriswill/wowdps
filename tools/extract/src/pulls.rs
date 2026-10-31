//! The history store's fights, read in one place for the tools over them:
//! what the replay's extractor reads of the replay tier
//! (wowdps-replay-extract: gen-rubric's census in its `rubricgen`, its
//! stencil tracer, its level posts, its turns and its cut; its `posts`
//! holds the positions and level posts files).
//!
//! The store is `$XDG_DATA_HOME/wowdps/history/v1` (or wherever `--store`
//! / `--census` points), the daemon's alone to write: a card a fight
//! (`fights/<id>.json`, `FightCard`) and, for a fight that keeps one, its
//! replay tier (`replay/<id>.bin`, `wowdps_proto::replay`). A fight is
//! named by its id; a tool over every fight reads each replay this build
//! decodes and passes over (and counts) the rest.
//!
//! A fight's encounters are its windows (`Fight::windows`): a boss pull is
//! one encounter over its whole cut; a keystone run holds one per boss it
//! engaged, from the boss row that engaged it to the one that ended it (a
//! kill or a wipe of the same encounter), else to the cut's end.

use std::collections::BTreeSet;
use std::path::Path;

use wowdps_core::model::replay::{Cut, EventKind, Unit, UnitKind};
use wowdps_proto::history::{FightCard, FightKind};

/// The ids of the fights whose replay tier the store at `store` keeps
/// (`replay/<id>.bin`), sorted. An unreadable store is an error.
pub fn replay_ids(store: &Path) -> Result<Vec<String>, String> {
    let dir = store.join("replay");
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".bin").map(str::to_string)
        })
        .collect();
    ids.sort();
    Ok(ids)
}

/// A stored fight's card, where it reads.
pub fn read_card(store: &Path, id: &str) -> Option<FightCard> {
    let text = std::fs::read_to_string(store.join("fights").join(format!("{id}.json"))).ok()?;
    FightCard::from_json(&wowdps_proto::json::parse(&text).ok()?)
}

/// One stored fight: its id, its card (where it reads) and its replay
/// tier's cut.
#[derive(Debug, Clone, PartialEq)]
pub struct Fight {
    pub id: String,
    pub card: Option<FightCard>,
    pub cut: Cut,
}

/// One encounter of a fight: its DungeonEncounterID and the stretch of the
/// cut it spans (ms on the cut's clock; `until` none: to the cut's end).
/// `whole` for a boss pull's, whose units are all the cut's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub encounter: u32,
    pub from: u32,
    pub until: Option<u32>,
    pub whole: bool,
}

impl Window {
    /// Whether `t` (ms) falls in it.
    pub fn holds(&self, t: u32) -> bool {
        self.whole || (t >= self.from && self.until.is_none_or(|u| t <= u))
    }
}

impl Fight {
    /// The fight `id` of the store at `store`: its replay tier decoded, its
    /// card beside it where it reads. A fight that keeps no replay, or one
    /// this build cannot decode, is an error naming it.
    pub fn read(store: &Path, id: &str) -> Result<Fight, String> {
        let path = store.join("replay").join(format!("{id}.bin"));
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cut = wowdps_proto::replay::decode(&bytes).ok_or_else(|| {
            format!(
                "{}: not a replay this build reads (format {:?})",
                path.display(),
                wowdps_proto::replay::format_of(&bytes)
            )
        })?;
        Ok(Fight {
            id: id.to_string(),
            card: read_card(store, id),
            cut,
        })
    }

    /// Whether it is a keystone run: its card says so, else its cut names a
    /// key and no encounter.
    pub fn is_key(&self) -> bool {
        match &self.card {
            Some(c) => c.kind == FightKind::Key,
            None => self.cut.head.encounter.is_none() && self.cut.head.key.is_some(),
        }
    }

    /// Its encounters: a boss pull's one, over the whole cut (its cut's
    /// encounter, else its card's); a keystone run's, one per boss row that
    /// engaged a boss, to the row that ended that encounter (killed or
    /// wiped), else to the cut's end.
    pub fn windows(&self) -> Vec<Window> {
        if !self.is_key() {
            let id = self
                .cut
                .head
                .encounter
                .as_ref()
                .map(|e| e.id)
                .or_else(|| self.card.as_ref()?.encounter.map(|e| e.id));
            return id
                .map(|encounter| Window {
                    encounter,
                    from: 0,
                    until: None,
                    whole: true,
                })
                .into_iter()
                .collect();
        }
        let events = &self.cut.events;
        events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == EventKind::BossEngaged)
            .map(|(i, e)| Window {
                encounter: e.spell_id,
                from: e.t_ms,
                until: events
                    .iter()
                    .skip(i + 1)
                    .find(|x| {
                        matches!(x.kind, EventKind::BossKilled | EventKind::BossWiped)
                            && x.spell_id == e.spell_id
                    })
                    .map(|x| x.t_ms),
                whole: false,
            })
            .collect()
    }

    /// Its windows of encounter `id`.
    pub fn windows_of(&self, id: u32) -> Vec<Window> {
        self.windows()
            .into_iter()
            .filter(|w| w.encounter == id)
            .collect()
    }

    /// The units of window `w`, in the cut's order: every one of a whole
    /// cut; else each with a post in it, or named by an event in it (as
    /// its `unit`, `src` or `target`).
    pub fn units_in(&self, w: &Window) -> Vec<&Unit> {
        if w.whole {
            return self.cut.units.iter().collect();
        }
        let named: BTreeSet<u32> = self
            .cut
            .events
            .iter()
            .filter(|e| w.holds(e.t_ms))
            .flat_map(|e| [e.unit, e.src, e.target])
            .flatten()
            .collect();
        self.cut
            .units
            .iter()
            .enumerate()
            .filter(|(i, u)| {
                u32::try_from(*i).is_ok_and(|i| named.contains(&i))
                    || u.posts.iter().any(|p| w.holds(p.t_ms))
            })
            .map(|(_, u)| u)
            .collect()
    }

    /// Every player's post in window `w`, as world X and Y in yards (the
    /// cut keeps hundredths), unit by unit, each in time order.
    pub fn player_positions(&self, w: &Window) -> Vec<[f32; 2]> {
        self.cut
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Player)
            .flat_map(|u| u.posts.iter())
            .filter(|p| w.holds(p.t_ms))
            .map(|p| [p.x as f32 / 100.0, p.y as f32 / 100.0])
            .collect()
    }
}

/// Whether a fight whose card is `card` may hold encounter `id`: a boss
/// pull of it, or a keystone run whose boss list names it. Without a card
/// it may.
fn may_hold(card: Option<&FightCard>, id: u32) -> bool {
    card.is_none_or(|c| {
        c.encounter.is_some_and(|e| e.id == id)
            || c.kind == FightKind::Key
                && c.bosses
                    .iter()
                    .any(|b| b.encounter.is_some_and(|e| e.id == id))
    })
}

/// What a walk over the store read: the fights it decoded and the replay
/// files it passed over (a format this build does not read, a file that
/// will not read).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Walked {
    pub read: usize,
    pub passed_over: usize,
}

/// Every fight of the store at `store` that keeps a replay, in the order of
/// their ids, handed to `f` one at a time; with `encounter`, only those
/// whose card says they may hold it (a boss pull of it, a run whose
/// bosses name it; a fight without a card is read). A replay this build
/// cannot decode is passed over and counted.
pub fn walk(
    store: &Path,
    encounter: Option<u32>,
    mut f: impl FnMut(&Fight) -> Result<(), String>,
) -> Result<Walked, String> {
    let mut walked = Walked::default();
    for id in replay_ids(store)? {
        if let Some(e) = encounter
            && !may_hold(read_card(store, &id).as_ref(), e)
        {
            continue;
        }
        match Fight::read(store, &id) {
            Ok(fight) => {
                walked.read += 1;
                f(&fight)?;
            }
            Err(_) => walked.passed_over += 1,
        }
    }
    Ok(walked)
}

/// Every player's position in each stored fight's windows of encounter
/// `id` (its boss pulls whole, its keystone runs' fights of it): a kill and
/// its wipes together, and how many fights gave any.
pub fn encounter_positions(store: &Path, id: u32) -> Result<(Vec<[f32; 2]>, usize), String> {
    let (mut out, mut fights) = (Vec::new(), 0);
    walk(store, Some(id), |fight| {
        let before = out.len();
        for w in fight.windows_of(id) {
            out.extend(fight.player_positions(&w));
        }
        if out.len() > before {
            fights += 1;
        }
        Ok(())
    })?;
    Ok((out, fights))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wowdps_core::model::Encounter;
    use wowdps_core::model::replay::Event;

    /// A unit posted at each `(map, x, y)` in yards, in order.
    fn posted(kind: UnitKind, posts: &[(u32, f32, f32)]) -> wowdps_core::model::replay::Unit {
        wowdps_core::model::replay::Unit {
            kind,
            name: "Someone-Realm-US".into(),
            guid: String::new(),
            class: None,
            spec: None,
            you: false,
            npc: 0,
            posts: posts
                .iter()
                .enumerate()
                .map(|(i, &(map_id, x, y))| wowdps_core::model::replay::Post {
                    t_ms: i as u32 * 1000,
                    x: (x * 100.0) as i32,
                    y: (y * 100.0) as i32,
                    facing: 0,
                    hp: 1000,
                    power: None,
                    absorb: 0,
                    pools: Vec::new(),
                    map_id,
                })
                .collect(),
        }
    }

    /// A row of `kind` at `t`: a boss row (no unit, the encounter as its
    /// spell), else a cast by `unit` at `target`.
    fn row(
        t_ms: u32,
        kind: EventKind,
        spell_id: u32,
        unit: Option<u32>,
        target: Option<u32>,
    ) -> Event {
        Event {
            t_ms,
            kind,
            unit,
            spell_id,
            spell: String::new(),
            at: None,
            src: None,
            stacks: None,
            base: None,
            target,
        }
    }

    /// A store directory of its own, emptied when dropped.
    struct Store(std::path::PathBuf);

    impl Store {
        fn new(tag: &str) -> Store {
            let dir =
                std::env::temp_dir().join(format!("wowdps-pulls-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("fights")).unwrap();
            std::fs::create_dir_all(dir.join("replay")).unwrap();
            Store(dir)
        }

        /// The fight `id`: its card (where given) and its replay tier.
        fn put(&self, id: &str, card: Option<FightCard>, cut: &Cut) {
            if let Some(mut card) = card {
                card.id = id.to_string();
                let mut text = String::new();
                card.to_json().write(&mut text);
                std::fs::write(self.0.join("fights").join(format!("{id}.json")), text).unwrap();
            }
            std::fs::write(
                self.0.join("replay").join(format!("{id}.bin")),
                wowdps_proto::replay::encode(cut),
            )
            .unwrap();
        }
    }

    impl Drop for Store {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn boss_card(id: u32) -> FightCard {
        FightCard {
            kind: FightKind::Encounter,
            encounter: Some(Encounter {
                id,
                difficulty: 16,
                group_size: 20,
            }),
            ..FightCard::default()
        }
    }

    /// A keystone run of two bosses (2124 killed at 3 s, 2125 engaged at
    /// 5 s and never ended), its players posted through it: each window's
    /// units and positions are those inside it.
    fn key_cut() -> Cut {
        let mut boss_a = posted(UnitKind::Boss, &[(1, 1.0, 1.0), (1, 2.0, 2.0)]);
        boss_a.name = "Adderis".into();
        boss_a.npc = 133379;
        let mut trash = posted(UnitKind::Add, &[]);
        trash.name = "Trash".into();
        trash.npc = 5;
        let mut boss_b = posted(UnitKind::Boss, &[]);
        boss_b.name = "Merektha".into();
        boss_b.npc = 133384;
        Cut {
            units: vec![
                posted(
                    UnitKind::Player,
                    &[
                        (1, 10.0, 10.0),
                        (1, 11.0, 10.0),
                        (1, 12.0, 10.0),
                        (1, 13.0, 10.0),
                        (1, 14.0, 10.0),
                        (1, 15.0, 10.0),
                        (1, 16.0, 10.0),
                    ],
                ),
                boss_a,
                trash,
                boss_b,
            ],
            events: vec![
                row(1000, EventKind::BossEngaged, 2124, None, None),
                row(2000, EventKind::CastSuccess, 1, Some(1), Some(0)),
                row(3000, EventKind::BossKilled, 2124, None, None),
                row(4000, EventKind::CastSuccess, 1, Some(2), Some(0)),
                row(5000, EventKind::BossEngaged, 2125, None, None),
                row(5500, EventKind::CastStart, 2, Some(3), None),
            ],
            ..Cut::default()
        }
    }

    #[test]
    fn a_runs_windows_span_its_boss_rows() {
        let store = Store::new("key");
        let card = FightCard {
            kind: FightKind::Key,
            ..FightCard::default()
        };
        store.put("k1", Some(card), &key_cut());
        let fight = Fight::read(&store.0, "k1").unwrap();
        assert!(fight.is_key());
        let w = fight.windows();
        assert_eq!(
            w,
            [
                Window {
                    encounter: 2124,
                    from: 1000,
                    until: Some(3000),
                    whole: false
                },
                Window {
                    encounter: 2125,
                    from: 5000,
                    until: None,
                    whole: false
                },
            ]
        );
        let names = |w: &Window| -> Vec<String> {
            fight.units_in(w).iter().map(|u| u.name.clone()).collect()
        };
        // The player posted, the boss posted and casting; the trash's cast
        // at 4 s falls between the two.
        assert_eq!(names(&w[0]), ["Someone-Realm-US", "Adderis"]);
        assert_eq!(names(&w[1]), ["Someone-Realm-US", "Merektha"]);
        assert_eq!(
            fight.player_positions(&w[0]),
            [[11.0, 10.0], [12.0, 10.0], [13.0, 10.0]]
        );
        assert_eq!(fight.player_positions(&w[1]), [[15.0, 10.0], [16.0, 10.0]]);
    }

    /// A boss pull is one window, whole: every unit and every player's post;
    /// the encounter from its cut, else its card.
    #[test]
    fn a_boss_pull_is_one_window_whole() {
        let store = Store::new("boss");
        let mut cut = key_cut();
        cut.events.clear();
        store.put("b1", Some(boss_card(3492)), &cut);
        let fight = Fight::read(&store.0, "b1").unwrap();
        assert!(!fight.is_key());
        let w = fight.windows();
        assert_eq!(
            w,
            [Window {
                encounter: 3492,
                from: 0,
                until: None,
                whole: true
            }]
        );
        assert_eq!(fight.units_in(&w[0]).len(), 4);
        assert_eq!(fight.player_positions(&w[0]).len(), 7);
        // No card, and no encounter in the cut: no window.
        store.put("b2", None, &cut);
        assert!(Fight::read(&store.0, "b2").unwrap().windows().is_empty());
    }

    /// A walk reads every fight that keeps a replay in id order, passes over
    /// (and counts) one this build cannot decode, and with an encounter
    /// skips the cards that cannot hold it; an encounter's positions come
    /// from its boss pulls and its runs' fights of it alike.
    #[test]
    fn a_walk_reads_the_store_and_counts_what_it_passes_over() {
        let store = Store::new("walk");
        let mut pull = key_cut();
        pull.events.clear();
        store.put("a-boss", Some(boss_card(2124)), &pull);
        store.put("b-other", Some(boss_card(9999)), &pull);
        let run = FightCard {
            kind: FightKind::Key,
            bosses: vec![wowdps_proto::history::KeyBoss {
                name: "Adderis".into(),
                encounter: Some(Encounter {
                    id: 2124,
                    difficulty: 8,
                    group_size: 5,
                }),
                ..Default::default()
            }],
            ..FightCard::default()
        };
        store.put("c-run", Some(run), &key_cut());
        std::fs::write(store.0.join("replay").join("d-bad.bin"), b"WDRP\x09").unwrap();
        assert_eq!(
            replay_ids(&store.0).unwrap(),
            ["a-boss", "b-other", "c-run", "d-bad"]
        );
        let mut seen = Vec::new();
        let walked = walk(&store.0, None, |f| {
            seen.push(f.id.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, ["a-boss", "b-other", "c-run"]);
        assert_eq!(
            walked,
            Walked {
                read: 3,
                passed_over: 1
            }
        );
        let mut seen = Vec::new();
        walk(&store.0, Some(2124), |f| {
            seen.push(f.id.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, ["a-boss", "c-run"]);
        let (positions, fights) = encounter_positions(&store.0, 2124).unwrap();
        assert_eq!(fights, 2);
        assert_eq!(positions.len(), 7 + 3);
        assert_eq!(encounter_positions(&store.0, 1).unwrap(), (Vec::new(), 0));
        assert!(
            Fight::read(&store.0, "d-bad")
                .unwrap_err()
                .contains("not a replay")
        );
        assert!(Fight::read(&store.0, "none").is_err());
        assert!(replay_ids(&store.0.join("nowhere")).is_err());
    }
}
