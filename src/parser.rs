//! WoW advanced combat log line parser.
//!
//! Layout is verified against the WowCoach.gg machine-readable spec
//! (`format_version: 22`, `verified_against_patch: "12.0+"`), cross-checked against
//! warcraft.wiki.gg. Where the two disagree the spec wins — see `design-core.md`.
//!
//! Field indices below are into the comma-split of the text *after* the timestamp:
//! `0` is the event name, `1..=8` the base unit block. There is **no** `hideCaster`
//! field in the file format (that is the in-game API shape only).

/// Number of fields in the advanced-combat-logging block. The wiki says 17; that is
/// wrong for current retail — two always-zero fields sit between `absorb` and
/// `power_type`.
const ADVANCED_LEN: usize = 19;

const FLAG_TYPE_PLAYER: u32 = 0x0000_0400;
const FLAG_TYPE_PET: u32 = 0x0000_1000;
const FLAG_TYPE_GUARDIAN: u32 = 0x0000_2000;

/// A parsed log line. `ts_ms` is monotonic within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub ts_ms: i64,
    pub event: Event,
    /// Pet ownership learned from the advanced block, when it carries one. Additive to
    /// the contract (the contract mandates advanced-field `ownerGUID` attribution but
    /// `Event` has no slot for it). Callers that only care about `event` can ignore it.
    pub owner_hint: Option<OwnerHint>,
}

impl LogLine {
    pub fn new(ts_ms: i64, event: Event) -> Self {
        Self { ts_ms, event, owner_hint: None }
    }
}

/// "`unit_guid` is owned by `owner_guid`", as reported by the advanced block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerHint {
    pub unit_guid: String,
    pub owner_guid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unit {
    pub guid: String,
    pub name: String,
    pub flags: u32,
}

impl Unit {
    pub fn is_player(&self) -> bool {
        self.flags & FLAG_TYPE_PLAYER != 0 || self.guid.starts_with("Player-")
    }

    pub fn is_pet_or_guardian(&self) -> bool {
        self.flags & (FLAG_TYPE_PET | FLAG_TYPE_GUARDIAN) != 0 || self.guid.starts_with("Pet-")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spell {
    pub id: u32,
    pub name: String,
    pub school: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraType {
    Buff,
    Debuff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Version {
        log_version: u32,
        advanced: bool,
    },
    EncounterStart {
        id: u32,
        name: String,
        difficulty: u32,
        group_size: u32,
    },
    EncounterEnd {
        id: u32,
        name: String,
        success: bool,
    },
    CombatantInfo {
        guid: String,
    },
    Damage {
        src: Unit,
        dst: Unit,
        spell: Option<Spell>,
        amount: u64,
        overkill: i64,
        absorbed: u64,
        critical: bool,
        periodic: bool,
    },
    Heal {
        src: Unit,
        dst: Unit,
        spell: Spell,
        /// Total healing *including* overheal (the canonical log value).
        amount: u64,
        overheal: u64,
        absorbed: u64,
        critical: bool,
    },
    Absorbed {
        src: Unit,
        dst: Unit,
        absorber: Unit,
        /// The damage spell that got absorbed; `None` when the hit was a melee swing.
        spell: Option<Spell>,
        /// The shield spell doing the absorbing (always present).
        absorb_spell: Spell,
        amount: u64,
    },
    Interrupt {
        src: Unit,
        dst: Unit,
        spell: Spell,
        interrupted_spell: Spell,
    },
    AuraApplied {
        src: Unit,
        dst: Unit,
        spell: Spell,
        aura_type: AuraType,
    },
    Dispel {
        src: Unit,
        dst: Unit,
        spell: Spell,
        dispelled_spell: Spell,
    },
    Summon {
        owner: Unit,
        pet: Unit,
    },
    Death {
        unit: Unit,
    },
    /// Recognised as a log line but not modelled. Never an error.
    Other,
}

/// Parse one log line. `None` for blank or malformed lines; unknown events yield
/// `Event::Other`. Never panics.
pub fn parse_line(_line: &str) -> Option<LogLine> {
    todo!("parse_line")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "7/27/2026 21:03:11.472-4";

    /// The 19-field advanced block, parameterised by the unit it describes and its owner.
    fn adv(info: &str, owner: &str) -> String {
        format!("{info},{owner},125000,180000,4200,0,8500,0,0,0,3,95,100,0,1234.56,-987.65,2222,3.14,639")
    }

    fn line(body: &str) -> String {
        format!("{TS}  {body}")
    }

    fn parse(body: &str) -> Event {
        parse_line(&line(body)).expect("should parse").event
    }

    const PLAYER: &str = r#"Player-1168-0A234B,"Thrall-Ragnaros",0x511,0x0"#;
    const HEALER: &str = r#"Player-1168-0B999C,"Moira-Ragnaros",0x512,0x0"#;
    const BOSS: &str =
        r#"Creature-0-4232-2662-31585-214502-0001,"Ulgrax the Devourer",0xa48,0x0"#;
    const BOSS_GUID: &str = "Creature-0-4232-2662-31585-214502-0001";
    const NIL_UNIT: &str = "0000000000000000,nil,0x80000000,0x0";

    // ---- timestamps -------------------------------------------------------

    #[test]
    fn parses_retail_timestamp_with_tz_offset() {
        let a = parse_line(&line("SPELL_CAST_SUCCESS,x")).unwrap();
        let b = parse_line("7/27/2026 21:03:12.472-4  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 1000, "one second apart");
    }

    #[test]
    fn parses_legacy_timestamp_without_year_or_tz() {
        let a = parse_line("7/27 21:03:11.000  SPELL_CAST_SUCCESS,x").unwrap();
        let b = parse_line("7/27 21:03:11.250  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 250);
    }

    #[test]
    fn timestamp_is_monotonic_across_midnight() {
        let a = parse_line("7/27/2026 23:59:59.000-4  SPELL_CAST_SUCCESS,x").unwrap();
        let b = parse_line("7/28/2026 00:00:01.000-4  SPELL_CAST_SUCCESS,x").unwrap();
        assert_eq!(b.ts_ms - a.ts_ms, 2000);
    }

    #[test]
    fn accepts_tab_separator() {
        assert!(parse_line("7/27/2026 21:03:11.472-4\tSPELL_CAST_SUCCESS,x").is_some());
    }

    // ---- metadata ---------------------------------------------------------

    #[test]
    fn parses_combat_log_version() {
        let e = parse("COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1");
        assert_eq!(e, Event::Version { log_version: 22, advanced: true });
    }

    #[test]
    fn parses_encounter_start() {
        let e = parse(r#"ENCOUNTER_START,2917,"Ulgrax the Devourer",14,20,2657"#);
        assert_eq!(
            e,
            Event::EncounterStart {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                difficulty: 14,
                group_size: 20,
            }
        );
    }

    #[test]
    fn parses_encounter_end_kill_and_wipe() {
        let kill = parse(r#"ENCOUNTER_END,2917,"Ulgrax the Devourer",14,20,1,183000"#);
        assert_eq!(
            kill,
            Event::EncounterEnd { id: 2917, name: "Ulgrax the Devourer".into(), success: true }
        );
        // Trailing duration_ms is optional and absent here.
        let wipe = parse(r#"ENCOUNTER_END,2917,"Ulgrax the Devourer",14,20,0"#);
        assert_eq!(
            wipe,
            Event::EncounterEnd { id: 2917, name: "Ulgrax the Devourer".into(), success: false }
        );
    }

    #[test]
    fn parses_combatant_info_guid() {
        // COMBATANT_INFO is a monster of nested brackets; we only need field 1, which
        // precedes all of them.
        let e = parse("COMBATANT_INFO,Player-1168-0A234B,0,7549,3591,[(1,2,3),(4,5,6)],[],(0,0)");
        assert_eq!(e, Event::CombatantInfo { guid: "Player-1168-0A234B".into() });
    }

    // ---- damage -----------------------------------------------------------

    #[test]
    fn parses_advanced_spell_damage() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,250,1,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage { src, spell, amount, overkill, absorbed, critical, periodic, .. } = e
        else {
            panic!("expected Damage, got {e:?}")
        };
        assert_eq!(src.name, "Thrall-Ragnaros");
        assert!(src.is_player());
        assert_eq!(spell.unwrap().name, "Fireball");
        // amount is base_amount (offset 31), NOT raw_amount (offset 32).
        assert_eq!(amount, 12345);
        assert_eq!(overkill, -1, "-1 means not a killing blow; meter clamps");
        assert_eq!(absorbed, 250);
        assert!(critical);
        assert!(!periodic);
    }

    #[test]
    fn parses_killing_blow_overkill() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},9000,9000,3500,4,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage { overkill, .. } = e else { panic!() };
        assert_eq!(overkill, 3500);
    }

    #[test]
    fn periodic_damage_is_flagged() {
        let e = parse(&format!(
            "SPELL_PERIODIC_DAMAGE,{PLAYER},{BOSS},172,\"Corruption\",0x20,{},800,800,-1,32,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage { amount, periodic, .. } = e else { panic!() };
        assert_eq!(amount, 800);
        assert!(periodic);
    }

    /// Advanced logging OFF: the GUID probe at the advanced slot must return false.
    #[test]
    fn parses_damage_without_advanced_block() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,5000,5200,-1,4,0,0,0,1,nil,nil"
        ));
        let Event::Damage { amount, critical, .. } = e else { panic!("got {e:?}") };
        assert_eq!(amount, 5000);
        assert!(critical);
    }

    /// A comma inside a quoted spell name must not split the field.
    #[test]
    fn handles_comma_inside_quoted_name() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},999001,\"Blessing of Might, Greater\",0x4,5000,5200,-1,4,0,0,0,nil,nil,nil"
        ));
        let Event::Damage { spell, amount, .. } = e else { panic!() };
        assert_eq!(spell.unwrap().name, "Blessing of Might, Greater");
        assert_eq!(amount, 5000, "amount must survive the embedded comma");
    }

    // ---- the SWING optional-trailing-field trap ---------------------------

    /// Main-hand swings OMIT `is_off_hand` entirely (38 fields); off-hand swings have it
    /// (39). This pair is the regression test proving suffix-from-end would be wrong.
    #[test]
    fn swing_damage_main_hand_and_off_hand_both_parse() {
        let main = parse(&format!(
            "SWING_DAMAGE,{PLAYER},{BOSS},{},2500,2500,-1,1,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { amount, spell, .. } = main else { panic!() };
        assert_eq!(amount, 2500);
        assert!(spell.is_none(), "swings have no spell prefix");

        let off = parse(&format!(
            "SWING_DAMAGE,{PLAYER},{BOSS},{},1200,1200,-1,1,0,0,0,nil,nil,nil,1",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { amount, .. } = off else { panic!() };
        assert_eq!(amount, 1200);
    }

    // ---- double-count traps (R1) ------------------------------------------

    #[test]
    fn swing_damage_landed_is_other() {
        let e = parse(&format!(
            "SWING_DAMAGE_LANDED,{PLAYER},{BOSS},{},2500,2500,-1,1,0,0,0,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        ));
        assert_eq!(e, Event::Other, "duplicate of SWING_DAMAGE");
    }

    #[test]
    fn support_events_are_other() {
        for ev in [
            "SPELL_DAMAGE_SUPPORT",
            "SPELL_PERIODIC_DAMAGE_SUPPORT",
            "RANGE_DAMAGE_SUPPORT",
            "SPELL_HEAL_SUPPORT",
        ] {
            let e = parse(&format!(
                "{ev},{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,0,1,nil,nil,Player-1168-0AEVOK",
                adv(BOSS_GUID, "0000000000000000")
            ));
            assert_eq!(e, Event::Other, "{ev} duplicates the underlying hit");
        }
    }

    #[test]
    fn damage_split_is_other() {
        let e = parse(&format!(
            "DAMAGE_SPLIT,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},100,100,-1,4,0,0,0,nil,nil,nil",
            adv(BOSS_GUID, "0000000000000000")
        ));
        assert_eq!(e, Event::Other);
    }

    // ---- healing ----------------------------------------------------------

    /// The heal amount is suffix[1] (`amount`), not suffix[0] (`healed_to_hp`).
    #[test]
    fn parses_advanced_spell_heal() {
        let e = parse(&format!(
            "SPELL_HEAL,{HEALER},{PLAYER},2061,\"Flash Heal\",0x2,{},140000,20000,5000,0,1",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Heal { src, dst, amount, overheal, critical, .. } = e else { panic!("{e:?}") };
        assert_eq!(src.name, "Moira-Ragnaros");
        assert_eq!(dst.name, "Thrall-Ragnaros");
        assert_eq!(amount, 20000, "canonical amount includes overheal");
        assert_eq!(overheal, 5000);
        assert!(critical);
    }

    #[test]
    fn parses_full_overheal() {
        let e = parse(&format!(
            "SPELL_PERIODIC_HEAL,{HEALER},{PLAYER},139,\"Renew\",0x2,{},180000,8000,8000,0,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Heal { amount, overheal, .. } = e else { panic!() };
        assert_eq!((amount, overheal), (8000, 8000));
    }

    // ---- SPELL_ABSORBED, both arities -------------------------------------

    #[test]
    fn parses_self_shield_absorb_19_fields() {
        let e = parse(&format!(
            "SPELL_ABSORBED,{BOSS},{PLAYER},{PLAYER},17,\"Power Word: Shield\",0x2,3000,9000,nil"
        ));
        let Event::Absorbed { absorber, spell, absorb_spell, amount, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(absorber.name, "Thrall-Ragnaros");
        assert!(spell.is_none(), "no damage spell on the 19-field form");
        assert_eq!(absorb_spell.name, "Power Word: Shield");
        assert_eq!(amount, 3000);
    }

    #[test]
    fn parses_shield_on_other_absorb_22_fields() {
        let e = parse(&format!(
            "SPELL_ABSORBED,{BOSS},{PLAYER},468731,\"Devouring Bite\",0x1,{HEALER},17,\"Power Word: Shield\",0x2,4500,12000,nil"
        ));
        let Event::Absorbed { src, dst, absorber, spell, absorb_spell, amount } = e else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Ulgrax the Devourer");
        assert_eq!(dst.name, "Thrall-Ragnaros");
        assert_eq!(absorber.name, "Moira-Ragnaros", "credit goes to the shield caster");
        assert_eq!(spell.unwrap().name, "Devouring Bite");
        assert_eq!(absorb_spell.name, "Power Word: Shield");
        assert_eq!(amount, 4500);
    }

    // ---- interrupt / dispel / aura ----------------------------------------

    #[test]
    fn parses_interrupt() {
        let e = parse(&format!(
            "SPELL_INTERRUPT,{PLAYER},{BOSS},57994,\"Wind Shear\",0x8,468999,\"Digestive Acid\",0x8"
        ));
        let Event::Interrupt { spell, interrupted_spell, .. } = e else { panic!("{e:?}") };
        assert_eq!(spell.name, "Wind Shear");
        assert_eq!(interrupted_spell.name, "Digestive Acid");
    }

    #[test]
    fn parses_dispel() {
        let e = parse(&format!(
            "SPELL_DISPEL,{HEALER},{PLAYER},527,\"Purify\",0x2,468888,\"Carnivorous Contest\",0x20,DEBUFF"
        ));
        let Event::Dispel { spell, dispelled_spell, .. } = e else { panic!("{e:?}") };
        assert_eq!(spell.name, "Purify");
        assert_eq!(dispelled_spell.name, "Carnivorous Contest");
    }

    #[test]
    fn parses_aura_applied_debuff() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{PLAYER},{BOSS},118,\"Polymorph\",0x40,DEBUFF"
        ));
        let Event::AuraApplied { spell, aura_type, .. } = e else { panic!("{e:?}") };
        assert_eq!(spell.id, 118);
        assert_eq!(aura_type, AuraType::Debuff);
    }

    /// The optional trailing absorb amount must not be mistaken for the aura type.
    #[test]
    fn parses_aura_applied_buff_with_trailing_amount() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{HEALER},{PLAYER},17,\"Power Word: Shield\",0x2,BUFF,45000"
        ));
        let Event::AuraApplied { aura_type, .. } = e else { panic!("{e:?}") };
        assert_eq!(aura_type, AuraType::Buff);
    }

    // ---- summon / death / environmental -----------------------------------

    #[test]
    fn parses_summon() {
        let e = parse(
            r#"SPELL_SUMMON,Player-1168-0C777D,"Gul-Ragnaros",0x511,0x0,Pet-0-4232-2662-31585-165189-0100AB,"Felhunter",0x1114,0x0,691,"Summon Felhunter",0x20"#,
        );
        let Event::Summon { owner, pet } = e else { panic!("{e:?}") };
        assert_eq!(owner.guid, "Player-1168-0C777D");
        assert_eq!(pet.name, "Felhunter");
        assert!(pet.is_pet_or_guardian());
    }

    /// A pet's SWING advanced block describes the SOURCE, so it carries the owner GUID.
    #[test]
    fn swing_advanced_block_yields_owner_hint() {
        let l = parse_line(&line(&format!(
            "SWING_DAMAGE,Pet-0-4232-2662-31585-165189-0200CD,\"Bloodfang\",0x1114,0x0,{BOSS},{},1500,1500,-1,1,0,0,0,nil,nil,nil",
            adv("Pet-0-4232-2662-31585-165189-0200CD", "Player-1168-0D555E")
        )))
        .unwrap();
        assert_eq!(
            l.owner_hint,
            Some(OwnerHint {
                unit_guid: "Pet-0-4232-2662-31585-165189-0200CD".into(),
                owner_guid: "Player-1168-0D555E".into(),
            })
        );
    }

    /// A zero owner GUID is "not a pet" and must not produce a hint.
    #[test]
    fn zero_owner_guid_yields_no_hint() {
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},1,1,-1,4,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        )))
        .unwrap();
        assert_eq!(l.owner_hint, None);
    }

    #[test]
    fn parses_unit_died_with_nil_source() {
        let e = parse(&format!("UNIT_DIED,{NIL_UNIT},{PLAYER}"));
        let Event::Death { unit } = e else { panic!("{e:?}") };
        assert_eq!(unit.name, "Thrall-Ragnaros");
        assert!(unit.is_player());
    }

    #[test]
    fn parses_unit_died_with_trailing_recap_id() {
        let e = parse(&format!("UNIT_DIED,{NIL_UNIT},{PLAYER},1"));
        assert!(matches!(e, Event::Death { .. }));
    }

    /// envType sits at offset 28, AFTER the advanced block — not at 9 as the wiki says.
    #[test]
    fn parses_environmental_damage() {
        let e = parse(&format!(
            "ENVIRONMENTAL_DAMAGE,{NIL_UNIT},{PLAYER},{},Falling,4000,4000,-1,1,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { src, amount, .. } = e else { panic!("{e:?}") };
        assert_eq!(amount, 4000, "envType must be skipped, not read as the amount");
        assert!(!src.is_player(), "null source belongs to nobody");
    }

    // ---- unit flags -------------------------------------------------------

    #[test]
    fn unit_classification() {
        let player = Unit { guid: "Player-1-A".into(), name: "P".into(), flags: 0x511 };
        assert!(player.is_player());
        assert!(!player.is_pet_or_guardian());

        let pet = Unit { guid: "Pet-0-1".into(), name: "Felhunter".into(), flags: 0x1114 };
        assert!(pet.is_pet_or_guardian());
        assert!(!pet.is_player());

        let boss = Unit { guid: "Creature-0-1".into(), name: "B".into(), flags: 0xa48 };
        assert!(!boss.is_player());
        assert!(!boss.is_pet_or_guardian());
    }

    // ---- negative control: never panic, never poison the stream ------------

    #[test]
    fn malformed_lines_return_none_without_panicking() {
        let bad = [
            "",
            "   ",
            "\n",
            "7/27/2026 21:03:42.000-4",
            "SPELL_DAMAGE,Player-1,\"x\",0x0,0x0",
            "not/a/timestamp here  SPELL_DAMAGE,a,b",
            "7/27/2026 21:03:42.000-4  ",
            "//  ,,,,",
            "7/27/2026 :: .-4  SPELL_DAMAGE,a",
        ];
        for b in bad {
            assert!(parse_line(b).is_none(), "expected None for {b:?}");
        }
    }

    #[test]
    fn unterminated_quote_is_malformed() {
        assert!(parse_line(&line("SPELL_DAMAGE,Player-1,\"unterminated,0x0")).is_none());
    }

    #[test]
    fn truncated_damage_line_is_not_a_panic() {
        // Real logs get cut mid-write when the game crashes.
        let l = parse_line(&line(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345",
            adv(BOSS_GUID, "0000000000000000")
        )));
        assert!(matches!(l, None | Some(LogLine { event: Event::Other, .. })));
    }

    #[test]
    fn unknown_event_is_other_not_none() {
        let e = parse(&format!("SPELL_CAST_START,{PLAYER},{BOSS},133,\"Fireball\",0x4"));
        assert_eq!(e, Event::Other);
        let e = parse("SOME_FUTURE_EVENT,1,2,3");
        assert_eq!(e, Event::Other);
    }
}
