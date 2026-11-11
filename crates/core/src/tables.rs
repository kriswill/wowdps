//! The generated tables a fight reads, by the edition of the game its log
//! names (`Segment::project_id`, through `Edition::counts_as`: a log that
//! names none is Retail's). Editions share spell, item and map ids with no
//! promise that an id means one thing in both — spell 8647 is a Rogue's in
//! one and a Monk's in the other — so each edition's tables are its own,
//! and a fight reads its own edition's alone. An edition with no table of
//! a kind answers that kind with nothing (no class, no item, no shield, no
//! role, no driver, no timers), never with another edition's entry; an id
//! a table does not hold answers the same. What the table would have added
//! is all that goes missing: every amount is still counted (CONTRACT.md R8).
//!
//! The one exception, an interim one: an edition with no open-world map
//! table of its own (a Classic client, an unlisted id) reads Retail's, as
//! every edition did before the tables were per edition. With no table at
//! all, every door would keep the difficulty the game stamped on it, and
//! a hearth out of a raid (`ZONE_CHANGE,1,"Kalimdor",9`) would open a visit
//! of its own (R10); Retail's continents and cities are a better guess
//! than none until that edition's own table is generated.
//!
//! Every lookup into a generated table goes through here, the meter's, the
//! index scanner's and the replay cut's alike, so the three cannot read
//! different editions' tables for one fight. A table generated for an
//! edition is one entry in that edition's [`Tables`].

use crate::roles::RoleSignal;
use wowdps_model::{Class, Edition, ItemKind, RoleSpellKind, Spec};

/// A keystone dungeon's (par, +2, +3) timers, in milliseconds.
pub(crate) type Pars = (i64, i64, i64);

/// R8: the class a spell names, and its spec when it is unique to one.
type ClassOf = fn(u32) -> Option<(Class, Option<Spec>)>;

/// One edition's generated tables: each kind's lookups, `None` for a kind
/// the edition has no table of.
pub(crate) struct Tables {
    /// R8 (`class_spells`): the one class a spell names, and its spec when
    /// it is unique to one.
    class: Option<ClassOf>,
    /// R12 and R26 (`item_spells`).
    items: Option<Items>,
    /// R20 (`absorb_spells`): whether a spell places a shield.
    absorb: Option<fn(u32) -> bool>,
    /// R18 (`role_spells`): the role a buff plays.
    role: Option<fn(u32) -> Option<RoleSpellKind>>,
    /// R26 (`proc_spells`): the spell that drives a proc, by name.
    procs: Option<fn(u32) -> Option<&'static str>>,
    /// R10 (`keystone_timers`).
    keystones: Option<Keystones>,
    /// R10 (`open_world_maps`): whether a map is open world.
    open_world: Option<fn(u32) -> bool>,
    /// R30 (`role_signals`): what a cast or a form says about a player's
    /// role. Only an edition whose log carries no spec has one: with it the
    /// meter keeps R30's ledger and infers each fight's roles; without it
    /// (Retail, whose role is its spec's) it keeps none and infers nothing.
    signals: Option<fn(u32) -> Option<RoleSignal>>,
}

/// An item table's two questions.
struct Items {
    /// The kind of item a spell comes from.
    kind: fn(u32) -> Option<ItemKind>,
    /// The one trinket a spell comes from: (item id, its name).
    trinket: fn(u32) -> Option<(u32, &'static str)>,
}

/// A keystone table's two questions.
struct Keystones {
    /// A dungeon's timers by CHALLENGE_MODE_START's challenge id.
    by_challenge: fn(u32) -> Option<Pars>,
    /// The timers of the one dungeon on a map.
    by_map: fn(u32) -> Option<Pars>,
}

/// Retail's: every kind.
static RETAIL: Tables = Tables {
    class: Some(crate::class_spells::resolve),
    items: Some(Items {
        kind: crate::item_spells::item_kind,
        trinket: crate::item_spells::trinket_of,
    }),
    absorb: Some(crate::absorb_spells::is_absorb_spell),
    role: Some(crate::role_spells::role_kind),
    procs: Some(crate::proc_spells::driver_of),
    keystones: Some(Keystones {
        by_challenge: crate::keystone_timers::pars_ms,
        by_map: crate::keystone_timers::pars_for_map,
    }),
    open_world: Some(crate::open_world_maps::open_world),
    signals: None,
};

/// Forever's: its classes (by the client's class masks, no spec), its
/// items, its keystones (none: it has no Mythic+), its open-world maps and
/// its role signals (R30: its log carries no spec, so roles are inferred).
/// Its shields, role spans and procs have no table yet.
static FOREVER: Tables = Tables {
    class: Some(crate::forever::class_spells::resolve),
    items: Some(Items {
        kind: crate::forever::item_spells::item_kind,
        trinket: crate::forever::item_spells::trinket_of,
    }),
    absorb: None,
    role: None,
    procs: None,
    keystones: Some(Keystones {
        by_challenge: crate::forever::keystone_timers::pars_ms,
        by_map: crate::forever::keystone_timers::pars_for_map,
    }),
    open_world: Some(crate::forever::open_world_maps::open_world),
    signals: Some(crate::forever::role_signals::signal),
};

/// An edition no table was generated for: a Classic client, or a game this
/// build does not know. It reads Retail's open-world maps for now (the
/// module's one exception) and nothing else.
static NONE: Tables = Tables {
    class: None,
    items: None,
    absorb: None,
    role: None,
    procs: None,
    keystones: None,
    open_world: Some(crate::open_world_maps::open_world),
    signals: None,
};

impl Tables {
    /// The tables of `edition` as it counts: Retail's for Retail and for a
    /// log that names no edition, Forever's for Forever, and for any other
    /// the table-less set, which reads Retail's open-world maps alone.
    pub(crate) fn of(edition: Edition) -> &'static Tables {
        match edition.counts_as() {
            Edition::Retail => &RETAIL,
            Edition::Forever => &FOREVER,
            Edition::Classic(_) | Edition::Unknown(_) => &NONE,
        }
    }

    /// [`Tables::of`] the edition a COMBAT_LOG_VERSION's `PROJECT_ID` names.
    pub(crate) fn of_project(project_id: u8) -> &'static Tables {
        Tables::of(Edition::from_project_id(project_id))
    }

    /// R8: the class a spell names, and its spec when it is unique to one.
    pub(crate) fn class(&self, spell_id: u32) -> Option<(Class, Option<Spec>)> {
        self.class.and_then(|f| f(spell_id))
    }

    /// R12: the kind of item a spell comes from.
    pub(crate) fn item_kind(&self, spell_id: u32) -> Option<ItemKind> {
        self.items.as_ref().and_then(|t| (t.kind)(spell_id))
    }

    /// R26: the one trinket a spell comes from, (item id, its name).
    pub(crate) fn trinket_of(&self, spell_id: u32) -> Option<(u32, &'static str)> {
        self.items.as_ref().and_then(|t| (t.trinket)(spell_id))
    }

    /// R20: whether a spell places a shield.
    pub(crate) fn is_absorb_spell(&self, spell_id: u32) -> bool {
        self.absorb.is_some_and(|f| f(spell_id))
    }

    /// R30: whether this edition infers roles (it has a role-signal table).
    pub(crate) fn infers_roles(&self) -> bool {
        self.signals.is_some()
    }

    /// R30: what a cast (or a form's aura) says about a player's role.
    pub(crate) fn role_signal(&self, spell_id: u32) -> Option<RoleSignal> {
        self.signals.and_then(|f| f(spell_id))
    }

    /// R18: the role a buff plays.
    pub(crate) fn role_kind(&self, spell_id: u32) -> Option<RoleSpellKind> {
        self.role.and_then(|f| f(spell_id))
    }

    /// R26: the name of the spell that drives a proc.
    pub(crate) fn driver_of(&self, spell_id: u32) -> Option<&'static str> {
        self.procs.and_then(|f| f(spell_id))
    }

    /// R10: a keystone dungeon's timers by its challenge id.
    pub(crate) fn pars_ms(&self, challenge_id: u32) -> Option<Pars> {
        self.keystones
            .as_ref()
            .and_then(|t| (t.by_challenge)(challenge_id))
    }

    /// R10: the timers of the one keystone dungeon on a map.
    pub(crate) fn pars_for_map(&self, map_id: u32) -> Option<Pars> {
        self.keystones.as_ref().and_then(|t| (t.by_map)(map_id))
    }

    /// R10: whether a map is open world.
    pub(crate) fn open_world(&self, map_id: u32) -> bool {
        self.open_world.is_some_and(|f| f(map_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every edition a log can name, by the tables it reads: Retail's (1,
    /// and 0, a log that names none), Forever's (18), and none at all (a
    /// Classic client's, an unlisted number).
    const RETAILS: [Edition; 2] = [Edition::Retail, Edition::Unknown(0)];
    const TABLELESS: [Edition; 3] = [
        Edition::Classic(2),
        Edition::Classic(14),
        Edition::Unknown(7),
    ];

    fn every_edition() -> Vec<Edition> {
        let mut all = RETAILS.to_vec();
        all.push(Edition::Forever);
        all.extend(TABLELESS);
        all
    }

    /// Ids no table holds: what a lookup of one answers, in every edition.
    const STRANGERS: [u32; 2] = [u32::MAX, u32::MAX - 1];

    #[test]
    fn an_edition_reads_its_own_tables_and_a_log_naming_none_reads_retails() {
        for e in RETAILS {
            assert!(std::ptr::eq(Tables::of(e), &RETAIL), "{e:?}");
        }
        assert!(std::ptr::eq(Tables::of(Edition::Forever), &FOREVER));
        for e in TABLELESS {
            assert!(std::ptr::eq(Tables::of(e), &NONE), "{e:?}");
        }
        assert!(std::ptr::eq(Tables::of_project(0), &RETAIL));
        assert!(std::ptr::eq(Tables::of_project(1), &RETAIL));
        assert!(std::ptr::eq(Tables::of_project(18), &FOREVER));
        assert!(std::ptr::eq(Tables::of_project(2), &NONE));
        assert!(std::ptr::eq(Tables::of_project(200), &NONE));
    }

    /// One id, two editions, two classes: Expose Armor is a Rogue's in
    /// Forever, while Retail's table gives the id to a Monk; neither
    /// answers for an edition with no table.
    #[test]
    fn an_id_means_what_its_own_editions_table_says() {
        assert_eq!(
            Tables::of(Edition::Forever).class(8647),
            Some((Class::Rogue, None))
        );
        assert_eq!(
            Tables::of(Edition::Retail).class(8647).map(|(c, _)| c),
            Some(Class::Monk)
        );
        for e in TABLELESS {
            assert_eq!(Tables::of(e).class(8647), None, "{e:?}");
        }
    }

    #[test]
    fn a_class_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).class(id), None, "{e:?} {id}");
            }
        }
    }

    #[test]
    fn an_item_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).item_kind(id), None, "{e:?} {id}");
                assert_eq!(Tables::of(e).trinket_of(id), None, "{e:?} {id}");
            }
        }
    }

    /// Forever's item table is its own: spell 47 comes from one of its
    /// trinkets, and from no item Retail's table knows.
    #[test]
    fn an_item_is_read_off_its_own_editions_table() {
        let forever = Tables::of(Edition::Forever);
        assert_eq!(forever.item_kind(47), Some(ItemKind::Trinket));
        assert_eq!(forever.trinket_of(47), Some((941, "Deprecated Speedstone")));
        assert_eq!(Tables::of(Edition::Retail).item_kind(47), None);
        assert_eq!(Tables::of(Edition::Classic(2)).item_kind(47), None);
    }

    #[test]
    fn an_absorb_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert!(!Tables::of(e).is_absorb_spell(id), "{e:?} {id}");
            }
        }
        // Power Word: Shield is Retail's shield; Forever has no table.
        assert!(Tables::of(Edition::Retail).is_absorb_spell(17));
        assert!(!Tables::of(Edition::Forever).is_absorb_spell(17));
        assert!(!Tables::of(Edition::Classic(2)).is_absorb_spell(17));
    }

    #[test]
    fn a_role_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).role_kind(id), None, "{e:?} {id}");
            }
        }
        // Shield Wall is Retail's defensive; Forever has no table.
        assert_eq!(
            Tables::of(Edition::Retail).role_kind(871),
            Some(RoleSpellKind::Defensive)
        );
        assert_eq!(Tables::of(Edition::Forever).role_kind(871), None);
        assert_eq!(Tables::of(Edition::Unknown(7)).role_kind(871), None);
    }

    /// R30: only Forever infers roles; Retail's role is its spec's, and an
    /// edition with no table of its own reads none of Forever's.
    #[test]
    fn a_role_signal_lookup_reads_forevers_table_alone() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).role_signal(id), None, "{e:?} {id}");
            }
            assert_eq!(Tables::of(e).infers_roles(), e == Edition::Forever, "{e:?}");
        }
        let forever = Tables::of(Edition::Forever);
        // Taunt, Shield Block, Revenge, Defensive Stance, Healing Wave.
        assert_eq!(forever.role_signal(355), Some(RoleSignal::Taunt));
        assert_eq!(forever.role_signal(2565), Some(RoleSignal::Shield));
        assert_eq!(forever.role_signal(6574), Some(RoleSignal::Mitigation));
        assert_eq!(forever.role_signal(71), Some(RoleSignal::Form));
        assert_eq!(forever.role_signal(331), Some(RoleSignal::Heal));
        // Earth Shock's plain rank is no taunt; its taunting one is.
        assert_eq!(forever.role_signal(8046), None);
        assert_eq!(forever.role_signal(1219379), Some(RoleSignal::Taunt));
        for e in [Edition::Retail, Edition::Unknown(7), Edition::Classic(2)] {
            assert_eq!(Tables::of(e).role_signal(355), None, "{e:?}");
        }
    }

    #[test]
    fn a_proc_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).driver_of(id), None, "{e:?} {id}");
            }
        }
        // A proc Retail's table curates; Forever has no table.
        assert!(Tables::of(Edition::Retail).driver_of(1261160).is_some());
        assert_eq!(Tables::of(Edition::Forever).driver_of(1261160), None);
    }

    #[test]
    fn a_keystone_lookup_of_an_id_no_table_holds_is_none() {
        for e in every_edition() {
            for id in STRANGERS.into_iter().chain([0]) {
                assert_eq!(Tables::of(e).pars_ms(id), None, "{e:?} {id}");
                assert_eq!(Tables::of(e).pars_for_map(id), None, "{e:?} {id}");
            }
        }
        // Temple of the Jade Serpent (challenge 2, map 960) has Retail's
        // timers; Forever has no Mythic+, so its table is empty.
        assert!(Tables::of(Edition::Retail).pars_ms(2).is_some());
        assert_eq!(Tables::of(Edition::Forever).pars_ms(2), None);
        assert_eq!(Tables::of(Edition::Forever).pars_for_map(960), None);
    }

    #[test]
    fn an_open_world_lookup_of_a_map_no_table_holds_is_false() {
        // Every edition reads some map table (an edition with none of its
        // own reads Retail's), and none of them holds these ids.
        for e in every_edition() {
            for id in STRANGERS {
                assert!(!Tables::of(e).open_world(id), "{e:?} {id}");
            }
        }
        // Eastern Kingdoms is open world in both editions' tables.
        assert!(Tables::of(Edition::Retail).open_world(0));
        assert!(Tables::of(Edition::Forever).open_world(0));
        // Dalaran City is 2980 in Forever's map table; Retail's does not
        // list the id.
        assert!(Tables::of(Edition::Forever).open_world(2980));
        assert!(!Tables::of(Edition::Retail).open_world(2980));
    }

    /// The interim exception: an edition with no open-world table of its
    /// own reads Retail's, map for map, and has no other kind.
    #[test]
    fn an_edition_with_no_map_table_reads_retails_open_world_maps() {
        let retail = Tables::of(Edition::Retail);
        for e in TABLELESS {
            let t = Tables::of(e);
            for map in [0, 1, 409, 530, 571, 2980, 2769, u32::MAX] {
                assert_eq!(t.open_world(map), retail.open_world(map), "{e:?} {map}");
            }
            assert!(t.class.is_none() && t.items.is_none() && t.absorb.is_none());
            assert!(t.signals.is_none());
            assert!(t.role.is_none() && t.procs.is_none() && t.keystones.is_none());
        }
        assert!(Tables::of(Edition::Classic(2)).open_world(1));
        assert!(!Tables::of(Edition::Classic(2)).open_world(409));
    }
}
