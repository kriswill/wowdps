use super::*;

#[test]
fn a_tables_ids_are_each_tuples_first_number_or_each_number() {
    let tuples = "/// doc\n#[rustfmt::skip]\nstatic TABLE: &[(u32, u8, u16)] = &[\n    (17,4,0), (53,3,261),\n    (66,7,0),\n];\nstatic OTHER: &[u32] = &[9];";
    assert_eq!(table_ids(tuples), vec![17, 53, 66]);
    let bare = "static TABLE: &[u32] = &[\n    17, 740, 1234,\n    3288,\n];";
    assert_eq!(table_ids(bare), vec![17, 740, 1234, 3288]);
    assert!(table_ids("no table here").is_empty());
}

#[test]
fn the_committed_tables_give_their_spells() {
    let (all, class) = curated_spells();
    // Feral Frenzy's button is a class spell (a talent); R20's first
    // absorb (17, Power Word: Shield) is in the set.
    assert!(class.contains(&274837));
    assert!(all.contains(&17) && all.len() > class.len());
}

#[test]
fn a_description_that_embeds_another_names_its_parent() {
    assert_eq!(embedded_parent("$@spelldesc274837"), Some(274837));
    assert_eq!(embedded_parent("  $@spelldesc1269619 more"), Some(1269619));
    assert_eq!(embedded_parent("Deals $s1 damage. $@spelldesc12"), None);
    assert_eq!(embedded_parent("$@spellname274837"), None);
    assert_eq!(embedded_parent("$@spelldesc"), None);
}

/// A trigger graph (child → parents), the passive ones and the names.
struct Graph {
    parents: HashMap<u32, Vec<u32>>,
    passive: &'static [u32],
    names: &'static [(u32, &'static str)],
}

impl Graph {
    /// `root_of` over it.
    fn root(&self, spell: u32) -> Option<(u32, bool)> {
        let passive = |id: u32| self.passive.contains(&id);
        let name = |id: u32| {
            self.names
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, s)| s.to_string())
        };
        root_of(spell, &self.parents, &passive, &name)
    }
}

fn graph(
    edges: &[(u32, u32)],
    passive: &'static [u32],
    names: &'static [(u32, &'static str)],
) -> Graph {
    let mut parents: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(child, parent) in edges {
        parents.entry(child).or_default().push(parent);
    }
    Graph {
        parents,
        passive,
        names,
    }
}

#[test]
fn an_effect_of_a_button_has_the_button_for_its_parent() {
    // Feral Frenzy: the damage spell embeds the button's description.
    let g = graph(&[(274838, 274837)], &[], &[]);
    assert_eq!(g.root(274838), Some((274837, false)));
    // The button itself is made by nothing.
    assert_eq!(g.root(274837), None);
}

#[test]
fn an_effect_of_a_passive_is_flagged_so() {
    // A trinket's equip aura (passive) procs a buff that triggers damage.
    let g = graph(&[(3, 2), (2, 1)], &[1], &[]);
    assert_eq!(g.root(3), Some((1, true)));
    assert_eq!(g.root(2), Some((1, true)));
}

#[test]
fn an_active_root_wins_then_the_same_name_then_the_nearest() {
    // Two roots, one passive: the active one.
    let g = graph(&[(9, 1), (9, 2)], &[1], &[]);
    assert_eq!(g.root(9), Some((2, false)));
    // Two active roots: the one named as the spell is, though further,
    // and the parent the first active spell on the way to it.
    let g = graph(
        &[(9, 1), (9, 5), (5, 4)],
        &[],
        &[(9, "Frenzy"), (4, "Frenzy"), (1, "Other")],
    );
    assert_eq!(g.root(9), Some((5, false)));
    let g = graph(
        &[(9, 1), (9, 4)],
        &[],
        &[(9, "Frenzy"), (4, "Frenzy"), (1, "Other")],
    );
    assert_eq!(g.root(9), Some((4, false)));
    // No name to match: the nearest, then the lowest id.
    let g = graph(&[(9, 7), (9, 5), (5, 4)], &[], &[]);
    assert_eq!(g.root(9), Some((7, false)));
}

#[test]
fn a_cycle_with_no_root_gives_no_parent() {
    let g = graph(&[(1, 2), (2, 1)], &[], &[]);
    assert_eq!(g.root(1), None);
}

#[test]
fn the_file_carries_its_stamp_and_its_records_by_id() {
    let rec = |id: u32, name: &str| Record {
        id,
        name: name.into(),
        school: 1,
        flags: PARENT_PASSIVE,
        charges: 2,
        cast_ms: 1500,
        range_yd: 40.0,
        cooldown_ms: 45_000,
        duration_ms: -1,
        parent: 7,
        cost: "30 energy".into(),
        desc: "Deals damage.".into(),
        journal: String::new(),
        marks: vec![104, 105],
    };
    let ids: BTreeSet<u32> = [5, 3].into_iter().collect();
    let s = stamp("12.1.0.69933", &ids, &[]);
    assert_ne!(s, stamp("12.1.0.69934", &ids, &[]));
    assert_ne!(s, stamp("12.1.0.69933", &[3].into_iter().collect(), &[]));
    // The census moves it: a fight more is another file.
    let fight = PlayerFight {
        spec: 104,
        applied: [5].into_iter().collect(),
        cast: BTreeSet::new(),
    };
    assert_ne!(s, stamp("12.1.0.69933", &ids, &[fight]));
    let bytes = encode(&[rec(5, "Five"), rec(3, "Three")], s);
    assert_eq!(read_stamp(&bytes), Some(s));
    assert_eq!(read_stamp(b"WDPI\x01\0\0\0"), None);
    assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
    // The index sorted by id, the first record at the body's start.
    let n = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    assert_eq!(n, 2);
    let first = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let at = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    assert_eq!((first, at), (3, 0));
    let body = 20 + 2 * 8;
    let parent = u32::from_le_bytes(bytes[body..body + 4].try_into().unwrap());
    let (flags, marks) = (bytes[body + 21], bytes[body + 23]);
    let name_len = u32::from_le_bytes(bytes[body + 24..body + 28].try_into().unwrap());
    assert_eq!((parent, flags, marks, name_len), (7, PARENT_PASSIVE, 2, 5));
    assert_eq!(&bytes[body + 28..body + 33], b"Three");
    // After the four strings (the cost's 9, the description's 13, no
    // journal), the two specs.
    let specs = body + 33 + (4 + 9) + (4 + 13) + 4;
    assert_eq!(&bytes[specs..specs + 4], &[104, 0, 105, 0]);
    let second = u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
    assert_eq!(body + second, specs + 4);
}

/// A record for `omitted_presses`: `id` of `name`, its parent and its
/// cooldown.
fn tip(id: u32, name: &str, parent: u32, cooldown_ms: u32, flags: u8) -> Record {
    Record {
        id,
        name: name.into(),
        parent,
        cooldown_ms,
        flags,
        ..Record::default()
    }
}

/// One player's fight: their spec, the auras they put on and the names
/// of their casts.
fn played(spec: u32, applied: &[u32], cast: &[&str]) -> PlayerFight {
    PlayerFight {
        spec,
        applied: applied.iter().copied().collect(),
        cast: cast.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn a_press_is_omitted_for_the_specs_that_put_its_effect_on_without_it() {
    // Feral Frenzy (1) and its damage (2); Shield Block (3) and its buff
    // (4); a poison (6) of a press (5) with no cooldown; an effect of a
    // passive (8 of 7); a parent of another name (10 of 9).
    let records = [
        tip(1, "Feral Frenzy", 0, 45_000, CLASS),
        tip(2, "Feral Frenzy", 1, 0, 0),
        tip(3, "Shield Block", 0, 1_000, CLASS),
        tip(4, "Shield Block", 3, 0, 0),
        tip(5, "Deadly Poison", 0, 0, CLASS),
        tip(6, "Deadly Poison", 5, 0, 0),
        tip(7, "A Rune", 0, 0, PASSIVE),
        tip(8, "A Rune", 7, 0, PARENT_PASSIVE),
        tip(9, "Judgment", 0, 11_000, CLASS),
        tip(10, "Greater Judgment", 9, 0, 0),
    ];
    let (guardian, feral, prot) = (104, 103, 73);
    let mut fights = vec![
        // The guardians never log the press; the ferals do, but for one
        // fight in which a proc put it on.
        played(guardian, &[2], &["Mangle"]),
        played(guardian, &[2], &[]),
        played(feral, &[2], &["Feral Frenzy"]),
        played(feral, &[2], &["Feral Frenzy", "Rake"]),
        played(feral, &[2], &["Rake"]),
        // A protection warrior logs Shield Block where its buff shows, but
        // once.
        played(prot, &[4], &["Shield Block"]),
        played(prot, &[4], &["Shield Block"]),
        played(prot, &[4], &[]),
        // Nothing else is a mark's candidate, however often.
        played(prot, &[6, 8, 10], &[]),
        // A spec unknown counts for nothing.
        played(0, &[4], &[]),
        played(0, &[4], &[]),
    ];
    let omitted = omitted_presses(&records, &fights);
    assert_eq!(
        omitted.into_iter().collect::<Vec<_>>(),
        vec![(2, vec![guardian as u16])]
    );
    // A tie is no omission: the press is logged as often as not.
    fights.push(played(prot, &[4], &[]));
    assert!(!omitted_presses(&records, &fights).contains_key(&4));
    fights.push(played(prot, &[4], &[]));
    assert_eq!(
        omitted_presses(&records, &fights).get(&4),
        Some(&vec![prot as u16])
    );
}

#[test]
fn a_description_parent_stands_alone_over_the_trigger_rows() {
    // Rune of Lingering's damage (3): triggered by another rune's dummy
    // (2), which nameless spells (1) trigger, and described as the passive
    // rune (9) that grants it.
    let descs: HashMap<u32, String> = [
        (3, "$@spelldesc9".to_string()),
        (2, "Deals damage.".to_string()),
    ]
    .into_iter()
    .collect();
    let p = parents_of([(2, 3), (1, 2), (1, 1), (4, 2)], &descs);
    assert_eq!(p.get(&3), Some(&vec![9]));
    assert_eq!(p.get(&2), Some(&vec![1, 4]));
    // A spell's trigger of itself is no parent.
    assert_eq!(p.get(&1), None);
}

#[test]
fn the_parent_is_the_first_press_on_the_way_to_an_active_root() {
    // Fire Breath's burn (357209) is described as Fire Breath (357208),
    // which a hero talent's active spell (448896) triggers: the burn's
    // parent is the button the log writes, not the root.
    let g = graph(&[(357209, 357208), (357208, 448896)], &[], &[]);
    assert_eq!(g.root(357209), Some((357208, false)));
    // A passive between them is passed over.
    let g = graph(&[(3, 2), (2, 1)], &[2], &[]);
    assert_eq!(g.root(3), Some((1, false)));
    // A root that is passive, with an active spell under it: a proc.
    let g = graph(&[(3, 2), (2, 1)], &[1], &[]);
    assert_eq!(g.root(3), Some((1, true)));
}
