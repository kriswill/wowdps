use super::*;

/// One record as `spelltipgen::encode` lays it out.
struct Rec<'a> {
    id: u32,
    parent: u32,
    cast_ms: u32,
    cooldown_ms: u32,
    duration_ms: i32,
    range_yd: f32,
    school: u8,
    flags: u8,
    charges: u8,
    strings: [&'a str; 4],
    marks: &'a [u16],
}

/// A file of `recs` (in the order given; the index sorted as written).
fn file(recs: &[Rec]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut index = Vec::new();
    for r in recs {
        index.push((r.id, body.len() as u32));
        for v in [r.parent, r.cast_ms, r.cooldown_ms] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(&r.duration_ms.to_le_bytes());
        body.extend_from_slice(&r.range_yd.to_le_bytes());
        body.extend_from_slice(&[r.school, r.flags, r.charges, r.marks.len() as u8]);
        for s in r.strings {
            body.extend_from_slice(&(s.len() as u32).to_le_bytes());
            body.extend_from_slice(s.as_bytes());
        }
        for m in r.marks {
            body.extend_from_slice(&m.to_le_bytes());
        }
    }
    let mut out = b"WDPT".to_vec();
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&0xABCDu64.to_le_bytes());
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    for (id, at) in index {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
    }
    out.extend_from_slice(&body);
    out
}

fn frenzy() -> Vec<u8> {
    file(&[
        Rec {
            id: 274837,
            parent: 0,
            cast_ms: 0,
            cooldown_ms: 45_000,
            duration_ms: 0,
            range_yd: 5.0,
            school: 1,
            flags: 8,
            charges: 0,
            strings: ["Feral Frenzy", "25 energy", "Unleash a furious frenzy.", ""],
            marks: &[],
        },
        Rec {
            id: 274838,
            parent: 274837,
            cast_ms: 0,
            cooldown_ms: 0,
            duration_ms: 6_000,
            range_yd: 50.0,
            school: 1,
            flags: 0,
            charges: 0,
            strings: ["Feral Frenzy", "", "Unleash a furious frenzy.", ""],
            marks: &[104, 105],
        },
        Rec {
            id: 430589,
            parent: 429532,
            cast_ms: 0,
            cooldown_ms: 0,
            duration_ms: -1,
            range_yd: 0.0,
            school: 0x48,
            flags: 2,
            charges: 0,
            strings: ["Atmospheric Exposure", "", "", "The journal says so."],
            marks: &[],
        },
    ])
}

#[test]
fn a_whole_file_serves_each_spell_and_none_for_another() {
    let tips = SpellTips::from_bytes(frenzy()).unwrap();
    assert_eq!(tips.len(), 3);
    let button = tips.get(274837).unwrap();
    assert_eq!(button.name, "Feral Frenzy");
    assert!(button.class && !button.passive && button.parent.is_none());
    assert_eq!(
        button.facts_line(),
        "25 energy · Instant · Melee range · 45 sec cooldown"
    );
    let effect = tips.get(274838).unwrap();
    assert_eq!(
        effect.parent,
        Some(Parent {
            id: 274837,
            passive: false
        })
    );
    assert_eq!(effect.duration(), Some(6_000));
    // A guardian's and a restoration druid's logs omit the press; a feral's
    // write it, and an unknown spec never omits one.
    assert_eq!(effect.marks, vec![104, 105]);
    assert!(effect.omits_press_for(104) && effect.omits_press_for(105));
    assert!(!effect.omits_press_for(103) && !effect.omits_press_for(0));
    assert!(!button.omits_press_for(104));
    let proc = tips.get(430589).unwrap();
    assert_eq!(
        proc.parent,
        Some(Parent {
            id: 429532,
            passive: true
        })
    );
    assert_eq!(proc.duration(), None);
    assert_eq!(proc.school_name().as_deref(), Some("Astral"));
    assert_eq!(proc.journal, "The journal says so.");
    assert!(tips.get(1).is_none());
}

#[test]
fn an_absent_or_garbage_file_opens_as_none() {
    let dir = std::env::temp_dir().join(format!("spell-tips-{}", std::process::id()));
    assert!(SpellTips::open_at(&dir.join("absent.bin")).is_none());
    assert!(SpellTips::from_bytes(Vec::new()).is_none());
    assert!(SpellTips::from_bytes(b"WDPI\x01\0\0\0".to_vec()).is_none());
    let mut future = frenzy();
    future[4] = 3;
    assert!(SpellTips::from_bytes(future).is_none());
}

#[test]
fn a_truncated_file_answers_none_and_never_panics() {
    let whole = frenzy();
    // Short of its index: refused whole.
    assert!(SpellTips::from_bytes(whole[..30].to_vec()).is_none());
    // Its index whole, its body cut: every record short of its end answers
    // none.
    let cut = whole.len() - 10;
    let tips = SpellTips::from_bytes(whole[..cut].to_vec()).unwrap();
    assert!(tips.get(430589).is_none());
    assert!(tips.get(274837).is_some());
    // A lying count is refused, not allocated.
    let mut lying = frenzy();
    lying[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(SpellTips::from_bytes(lying).is_none());
}

#[test]
fn an_index_out_of_order_is_refused() {
    let mut bytes = frenzy();
    // Swap the first two ids.
    let (a, b) = (bytes[20..24].to_vec(), bytes[28..32].to_vec());
    bytes[20..24].copy_from_slice(&b);
    bytes[28..32].copy_from_slice(&a);
    assert!(SpellTips::from_bytes(bytes).is_none());
}

#[test]
fn the_words_read_as_the_games_tooltips() {
    assert_eq!(time_words(1_500), "1.5 sec");
    assert_eq!(time_words(45_000), "45 sec");
    assert_eq!(time_words(90_000), "1.5 min");
    assert_eq!(time_words(120_000), "2 min");
    let base = SpellTip {
        id: 1,
        name: "Spell".into(),
        school: 0x20,
        passive: false,
        channel: false,
        class: false,
        gcd: true,
        empower: false,
        parent: None,
        cast_ms: 1_500,
        range_yd: 40.0,
        cooldown_ms: 30_000,
        charges: 2,
        duration_ms: 0,
        cost: String::new(),
        desc: String::new(),
        journal: String::new(),
        marks: Vec::new(),
    };
    assert_eq!(
        base.facts_line(),
        "1.5 sec cast · 40 yd range · 2 charges, 30 sec recharge"
    );
    let channel = SpellTip {
        channel: true,
        range_yd: 50_000.0,
        cooldown_ms: 0,
        ..base.clone()
    };
    assert_eq!(channel.facts_line(), "Channeled · Unlimited range");
    let held = SpellTip {
        empower: true,
        ..channel.clone()
    };
    assert_eq!(held.cast_words(), "Empowered");
    let passive = SpellTip {
        passive: true,
        range_yd: 0.0,
        cooldown_ms: 0,
        ..base
    };
    assert_eq!(passive.facts_line(), "Passive");
}

#[test]
fn a_success_is_a_press_unless_a_passive_alone_makes_it() {
    let tips = SpellTips::from_bytes(frenzy()).unwrap();
    // The button: a class spell with a cooldown and a cost.
    let button = tips.get(274837).unwrap();
    assert!(button.pressed() && !button.passive_success());
    // A hero talent's proc: a passive parent, no GCD, cooldown or cost.
    let proc = tips.get(430589).unwrap();
    assert!(!proc.pressed() && proc.passive_success());
    // A talent-swapped press (a passive's chain reaches it, but it starts
    // the GCD) stays.
    let swapped = SpellTip {
        gcd: true,
        ..proc.clone()
    };
    assert!(!swapped.passive_success());
    let passive = SpellTip {
        passive: true,
        parent: None,
        ..proc
    };
    assert!(passive.passive_success());
}
