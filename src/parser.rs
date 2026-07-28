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
// Only `is_pet_or_guardian` reads these, and that method is contract-mandated
// public API rather than something this binary calls — see its comment below.
#[allow(dead_code)]
const FLAG_TYPE_PET: u32 = 0x0000_1000;
#[allow(dead_code)]
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
        Self {
            ts_ms,
            event,
            owner_hint: None,
        }
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
    /// A GUID of all zeros means "no unit". Real logs emit such lines carrying PLAYER
    /// flags anyway, so the nil check must come BEFORE the flag test or the meter grows
    /// a phantom player row.
    fn is_nil(&self) -> bool {
        self.guid.is_empty() || self.guid == ZERO_GUID
    }

    pub fn is_player(&self) -> bool {
        !self.is_nil() && (self.flags & FLAG_TYPE_PLAYER != 0 || self.guid.starts_with("Player-"))
    }

    /// Flag bits only — never the GUID prefix. Player-summoned units are not always
    /// `Pet-` GUIDs (an Efflorescence totem is a `Creature-`), and conversely ownership
    /// is established by `SPELL_SUMMON`/`ownerGUID`, not by this predicate.
    // Required by CONTRACT.md's `impl Unit`, but nothing calls it: pet damage is
    // attributed through `SPELL_SUMMON`/`ownerGUID` in `meter.rs`, and the TUI
    // never inspects units at all. Kept as contract surface, exercised by the
    // tests below; the allow is what a binary crate needs for unused pub items.
    #[allow(dead_code)]
    pub fn is_pet_or_guardian(&self) -> bool {
        !self.is_nil() && self.flags & (FLAG_TYPE_PET | FLAG_TYPE_GUARDIAN) != 0
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
        /// currentSpecID (field 25 on real retail lines); the meter maps it to a class.
        spec_id: Option<u32>,
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

const ZERO_GUID: &str = "0000000000000000";

const GUID_PREFIXES: [&str; 7] = [
    "Player-",
    "Pet-",
    "Creature-",
    "Vehicle-",
    "GameObject-",
    "BattlePet-",
    "Vignette-",
];

/// True for anything that can occupy the advanced block's `info_guid` slot. Every field
/// that sits there when advanced logging is OFF is an integer or a bare keyword
/// (`BUFF`, `Falling`, a miss type), so this probe cannot false-positive.
fn is_guid(s: &str) -> bool {
    s == ZERO_GUID || GUID_PREFIXES.iter().any(|p| s.starts_with(p))
}

/// Quote-aware CSV split. `None` on an unterminated quote (a truncated line).
fn split_csv(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if in_quotes {
        return None;
    }
    out.push(cur);
    Some(out)
}

/// Days since the Unix epoch. Howard Hinnant's civil-date algorithm.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `M/D/YYYY HH:MM:SS.mmm-7` (retail) or `M/D HH:MM:SS.mmm` (legacy).
///
/// The timezone offset is deliberately ignored: it is constant within a file, and only
/// monotonic deltas are required. When the year is absent it defaults to a fixed value
/// — again, only deltas matter.
fn parse_timestamp(s: &str) -> Option<i64> {
    let (date, time) = s.trim().split_once(' ')?;

    let mut dp = date.split('/');
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let year: i64 = match dp.next() {
        Some(y) => y.parse().ok()?,
        None => 2000,
    };
    if dp.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Trim the trailing timezone offset ("-7", "+2", "-04:00").
    let time = match time.find(['+', '-']) {
        Some(i) => &time[..i],
        None => time,
    };
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let min: i64 = tp.next()?.parse().ok()?;
    let secs = tp.next()?;
    let (sec, millis) = match secs.split_once('.') {
        Some((s, frac)) => {
            let digits: String = frac
                .chars()
                .filter(|c| c.is_ascii_digit())
                .take(3)
                .collect();
            if digits.is_empty() {
                return None;
            }
            let scale = 10_i64.pow(3 - digits.len() as u32);
            (s.parse::<i64>().ok()?, digits.parse::<i64>().ok()? * scale)
        }
        None => (secs.parse::<i64>().ok()?, 0),
    };
    if hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    Some(
        days_from_civil(year, month, day) * 86_400_000
            + hour * 3_600_000
            + min * 60_000
            + sec * 1_000
            + millis,
    )
}

fn parse_u32(s: &str) -> u32 {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        s.parse().unwrap_or(0)
    }
}

fn parse_u64(s: &str) -> u64 {
    s.parse().unwrap_or(0)
}

fn parse_i64(s: &str) -> i64 {
    s.parse().unwrap_or(0)
}

/// Combat-log booleans are `1` / `nil`.
fn truthy(s: &str) -> bool {
    !matches!(s, "nil" | "0" | "")
}

fn get(f: &[String], i: usize) -> Option<&str> {
    f.get(i).map(|s| s.as_str())
}

/// Unit block: `guid, name, flags, raidFlags` at `i`.
fn unit_at(f: &[String], i: usize) -> Unit {
    let name = get(f, i + 1).unwrap_or_default();
    Unit {
        guid: get(f, i).unwrap_or_default().to_string(),
        // The literal `nil` stands in for "no unit"; don't surface it as a display name.
        name: if name == "nil" {
            String::new()
        } else {
            name.to_string()
        },
        flags: parse_u32(get(f, i + 2).unwrap_or_default()),
    }
}

/// Spell block: `id, name, school` at `i`.
fn spell_at(f: &[String], i: usize) -> Spell {
    Spell {
        id: parse_u32(get(f, i).unwrap_or_default()),
        name: get(f, i + 1).unwrap_or_default().to_string(),
        school: parse_u32(get(f, i + 2).unwrap_or_default()),
    }
}

fn aura_type(s: &str) -> AuraType {
    if s.eq_ignore_ascii_case("BUFF") {
        AuraType::Buff
    } else {
        AuraType::Debuff
    }
}

/// Events that restate damage already logged elsewhere. Counting them double-counts.
fn is_duplicate_event(ev: &str) -> bool {
    ev.ends_with("_SUPPORT")          // Augmentation Evoker: same hit, logged twice
        || ev == "SWING_DAMAGE_LANDED" // same swing as SWING_DAMAGE, target's view
        || ev == "DAMAGE_SPLIT"        // defensive mechanic, not offensive damage
        || ev == "SPELL_HEAL_ABSORBED"
}

fn is_damage_event(ev: &str) -> bool {
    matches!(
        ev,
        "SWING_DAMAGE"
            | "SPELL_DAMAGE"
            | "SPELL_PERIODIC_DAMAGE"
            | "RANGE_DAMAGE"
            | "SPELL_BUILDING_DAMAGE"
            | "DAMAGE_SHIELD"
            | "ENVIRONMENTAL_DAMAGE"
    )
}

/// Parse one log line. `None` for blank or malformed lines; unknown events yield
/// `Event::Other`. Never panics.
pub fn parse_line(line: &str) -> Option<LogLine> {
    let line = line.trim_end_matches(['\n', '\r']);
    if line.trim().is_empty() {
        return None;
    }

    // Timestamp and event CSV are separated by two spaces (a tab on some clients).
    let (idx, skip) = match (line.find("  "), line.find('\t')) {
        (Some(a), Some(b)) if b < a => (b, 1),
        (Some(a), _) => (a, 2),
        (None, Some(b)) => (b, 1),
        (None, None) => return None,
    };
    let ts_ms = parse_timestamp(&line[..idx])?;

    let rest = line[idx + skip..].trim_start();
    if rest.is_empty() {
        return None;
    }
    let f = split_csv(rest)?;
    if f[0].is_empty() {
        return None;
    }

    Some(parse_event(&f, ts_ms))
}

fn parse_event(f: &[String], ts_ms: i64) -> LogLine {
    let ev = f[0].as_str();
    let plain = |event| LogLine::new(ts_ms, event);

    // Metadata events carry no base unit block.
    match ev {
        "COMBAT_LOG_VERSION" => {
            return plain(Event::Version {
                log_version: parse_u32(get(f, 1).unwrap_or_default()),
                advanced: truthy(get(f, 3).unwrap_or_default()),
            });
        }
        "ENCOUNTER_START" => {
            return plain(Event::EncounterStart {
                id: parse_u32(get(f, 1).unwrap_or_default()),
                name: get(f, 2).unwrap_or_default().to_string(),
                difficulty: parse_u32(get(f, 3).unwrap_or_default()),
                group_size: parse_u32(get(f, 4).unwrap_or_default()),
            });
        }
        "ENCOUNTER_END" => {
            return plain(Event::EncounterEnd {
                id: parse_u32(get(f, 1).unwrap_or_default()),
                name: get(f, 2).unwrap_or_default().to_string(),
                // Offset 5; the trailing duration_ms at 6 is optional.
                success: truthy(get(f, 5).unwrap_or_default()),
            });
        }
        "COMBATANT_INFO" => {
            // Nested brackets and tuples follow, but fields 1 (guid) and 25
            // (currentSpecID, still before the first bracket) precede all of them.
            return plain(Event::CombatantInfo {
                guid: get(f, 1).unwrap_or_default().to_string(),
                spec_id: get(f, 25).and_then(|v| v.parse().ok()),
            });
        }
        _ => {}
    }

    if is_duplicate_event(ev) || f.len() < 9 {
        return plain(Event::Other);
    }

    // SPELL_ABSORBED has no advanced block and genuinely variable arity (19 vs 22
    // fields), so it is the one event indexed from the END. The relative offsets below
    // are identical in both forms — verified against real 19- and 22-field lines.
    if ev == "SPELL_ABSORBED" {
        let n = f.len();
        if n < 19 {
            return plain(Event::Other);
        }
        return plain(Event::Absorbed {
            src: unit_at(f, 1),
            dst: unit_at(f, 5),
            absorber: unit_at(f, n - 10),
            // The damage-spell block is present only in the longer form.
            spell: (n >= 22).then(|| spell_at(f, 9)),
            absorb_spell: spell_at(f, n - 6),
            amount: parse_u64(get(f, n - 3).unwrap_or_default()),
        });
    }

    // Locate the advanced block, then index the suffix FORWARD from it. Indexing from
    // the end is unsafe: SWING_DAMAGE omits `is_off_hand` on main-hand swings.
    let prefix_len = if ev.starts_with("SPELL_")
        || ev.starts_with("RANGE_")
        || ev.starts_with("DAMAGE_SHIELD")
    {
        3
    } else {
        0
    };
    let adv_start = 9 + prefix_len;
    let advanced = f.len() > adv_start && is_guid(&f[adv_start]);
    let suffix = adv_start + if advanced { ADVANCED_LEN } else { 0 };

    // The advanced block describes the SOURCE on SWING_DAMAGE but the TARGET on
    // SPELL_*/RANGE_* events. Keying the hint on info_guid is correct either way.
    let owner_hint = if advanced {
        let info = get(f, adv_start).unwrap_or_default();
        let owner = get(f, adv_start + 1).unwrap_or_default();
        (owner != ZERO_GUID && is_guid(owner)).then(|| OwnerHint {
            unit_guid: info.to_string(),
            owner_guid: owner.to_string(),
        })
    } else {
        None
    };
    let with_hint = |event| LogLine {
        ts_ms,
        event,
        owner_hint: owner_hint.clone(),
    };

    let spell = (prefix_len == 3).then(|| spell_at(f, 9));

    if is_damage_event(ev) {
        // ENVIRONMENTAL_DAMAGE prepends envType to the suffix.
        let s = suffix + usize::from(ev == "ENVIRONMENTAL_DAMAGE");
        let Some(amount) = get(f, s) else {
            return with_hint(Event::Other);
        };
        if f.len() <= s + 7 {
            return with_hint(Event::Other);
        }
        return with_hint(Event::Damage {
            src: unit_at(f, 1),
            dst: unit_at(f, 5),
            spell,
            // suffix[0] is base_amount (post-mitigation, canonical);
            // suffix[1] is raw_amount (pre-mitigation, diagnostics only).
            amount: parse_u64(amount),
            overkill: parse_i64(get(f, s + 2).unwrap_or_default()),
            absorbed: parse_u64(get(f, s + 6).unwrap_or_default()),
            critical: truthy(get(f, s + 7).unwrap_or_default()),
            periodic: ev.contains("_PERIODIC_"),
        });
    }

    match ev {
        "SPELL_HEAL" | "SPELL_PERIODIC_HEAL" => {
            // With the advanced block the suffix is 5 fields led by `healed_to_hp`,
            // which is NOT the heal amount (it is zero when a heal is fully converted
            // to a shield, e.g. Death Strike). Without it, `amount` leads.
            let h = suffix + usize::from(f.len() >= suffix + 5);
            if f.len() <= h + 3 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Heal {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                amount: parse_u64(get(f, h).unwrap_or_default()),
                overheal: parse_u64(get(f, h + 1).unwrap_or_default()),
                absorbed: parse_u64(get(f, h + 2).unwrap_or_default()),
                critical: truthy(get(f, h + 3).unwrap_or_default()),
            })
        }
        "SPELL_INTERRUPT" => {
            if f.len() <= suffix + 2 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Interrupt {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                interrupted_spell: spell_at(f, suffix),
            })
        }
        "SPELL_DISPEL" | "SPELL_STOLEN" => {
            if f.len() <= suffix + 2 {
                return with_hint(Event::Other);
            }
            with_hint(Event::Dispel {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                dispelled_spell: spell_at(f, suffix),
            })
        }
        "SPELL_AURA_APPLIED" => {
            let Some(kind) = get(f, suffix) else {
                return with_hint(Event::Other);
            };
            with_hint(Event::AuraApplied {
                src: unit_at(f, 1),
                dst: unit_at(f, 5),
                spell: spell.unwrap_or_default(),
                // The optional trailing absorb amount sits AFTER this; never read it
                // as a stack count.
                aura_type: aura_type(kind),
            })
        }
        "SPELL_SUMMON" => with_hint(Event::Summon {
            owner: unit_at(f, 1),
            pet: unit_at(f, 5),
        }),
        "UNIT_DIED" => with_hint(Event::Death {
            unit: unit_at(f, 5),
        }),
        _ => with_hint(Event::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "7/27/2026 21:03:11.472-4";

    /// The 19-field advanced block, parameterised by the unit it describes and its owner.
    fn adv(info: &str, owner: &str) -> String {
        format!(
            "{info},{owner},125000,180000,4200,0,8500,0,0,0,3,95,100,0,1234.56,-987.65,2222,3.14,639"
        )
    }

    fn line(body: &str) -> String {
        format!("{TS}  {body}")
    }

    fn parse(body: &str) -> Event {
        parse_line(&line(body)).expect("should parse").event
    }

    const PLAYER: &str = r#"Player-1168-0A234B,"Thrall-Ragnaros",0x511,0x0"#;
    const HEALER: &str = r#"Player-1168-0B999C,"Moira-Ragnaros",0x512,0x0"#;
    const BOSS: &str = r#"Creature-0-4232-2662-31585-214502-0001,"Ulgrax the Devourer",0xa48,0x0"#;
    const BOSS_GUID: &str = "Creature-0-4232-2662-31585-214502-0001";
    // Real logs use 0x80000000 (not 0x0) for "no raid marker" — that is > i32::MAX.
    const NIL_UNIT: &str = "0000000000000000,nil,0x80000000,0x80000000";

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
        let e =
            parse("COMBAT_LOG_VERSION,22,ADVANCED_LOG_ENABLED,1,BUILD_VERSION,12.0.0,PROJECT_ID,1");
        assert_eq!(
            e,
            Event::Version {
                log_version: 22,
                advanced: true
            }
        );
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
            Event::EncounterEnd {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                success: true
            }
        );
        // Trailing duration_ms is optional and absent here.
        let wipe = parse(r#"ENCOUNTER_END,2917,"Ulgrax the Devourer",14,20,0"#);
        assert_eq!(
            wipe,
            Event::EncounterEnd {
                id: 2917,
                name: "Ulgrax the Devourer".into(),
                success: false
            }
        );
    }

    #[test]
    fn parses_combatant_info_guid() {
        // COMBATANT_INFO is a monster of nested brackets; we only need fields 1 and 25,
        // which precede all of them. A short line (no spec field) parses with None.
        let e = parse("COMBATANT_INFO,Player-1168-0A234B,0,7549,3591,[(1,2,3),(4,5,6)],[],(0,0)");
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-1168-0A234B".into(),
                spec_id: None,
            }
        );
    }

    #[test]
    fn parses_combatant_info_spec_id_from_a_real_shaped_line() {
        // Real retail line shape (build 12.0.7): 24 stat fields after the guid, then
        // currentSpecID at field 25, then the talent brackets. Spec 70 = Ret Paladin.
        let e = parse(
            "COMBATANT_INFO,Player-5-0E9E6142,1,2129,217,26548,664,0,0,0,0,968,968,968,221,0,668,668,668,0,1062,73,73,73,2361,70,[(81523,102493,1)],[],(0,0)",
        );
        assert_eq!(
            e,
            Event::CombatantInfo {
                guid: "Player-5-0E9E6142".into(),
                spec_id: Some(70),
            }
        );
    }

    // ---- damage -----------------------------------------------------------

    #[test]
    fn parses_advanced_spell_damage() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,{},12345,13000,-1,4,0,0,250,1,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage {
            src,
            spell,
            amount,
            overkill,
            absorbed,
            critical,
            periodic,
            ..
        } = e
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
        let Event::Damage { overkill, .. } = e else {
            panic!()
        };
        assert_eq!(overkill, 3500);
    }

    #[test]
    fn periodic_damage_is_flagged() {
        let e = parse(&format!(
            "SPELL_PERIODIC_DAMAGE,{PLAYER},{BOSS},172,\"Corruption\",0x20,{},800,800,-1,32,0,0,0,nil,nil,nil,ST",
            adv(BOSS_GUID, "0000000000000000")
        ));
        let Event::Damage {
            amount, periodic, ..
        } = e
        else {
            panic!()
        };
        assert_eq!(amount, 800);
        assert!(periodic);
    }

    /// Advanced logging OFF: the GUID probe at the advanced slot must return false.
    #[test]
    fn parses_damage_without_advanced_block() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},133,\"Fireball\",0x4,5000,5200,-1,4,0,0,0,1,nil,nil"
        ));
        let Event::Damage {
            amount, critical, ..
        } = e
        else {
            panic!("got {e:?}")
        };
        assert_eq!(amount, 5000);
        assert!(critical);
    }

    /// A comma inside a quoted spell name must not split the field.
    #[test]
    fn handles_comma_inside_quoted_name() {
        let e = parse(&format!(
            "SPELL_DAMAGE,{PLAYER},{BOSS},999001,\"Blessing of Might, Greater\",0x4,5000,5200,-1,4,0,0,0,nil,nil,nil"
        ));
        let Event::Damage { spell, amount, .. } = e else {
            panic!()
        };
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
        let Event::Damage { amount, spell, .. } = main else {
            panic!()
        };
        assert_eq!(amount, 2500);
        assert!(spell.is_none(), "swings have no spell prefix");

        let off = parse(&format!(
            "SWING_DAMAGE,{PLAYER},{BOSS},{},1200,1200,-1,1,0,0,0,nil,nil,nil,1",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { amount, .. } = off else {
            panic!()
        };
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
        let Event::Heal {
            src,
            dst,
            amount,
            overheal,
            critical,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
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
        let Event::Heal {
            amount, overheal, ..
        } = e
        else {
            panic!()
        };
        assert_eq!((amount, overheal), (8000, 8000));
    }

    // ---- SPELL_ABSORBED, both arities -------------------------------------

    #[test]
    fn parses_self_shield_absorb_19_fields() {
        let e = parse(&format!(
            "SPELL_ABSORBED,{BOSS},{PLAYER},{PLAYER},17,\"Power Word: Shield\",0x2,3000,9000,nil"
        ));
        let Event::Absorbed {
            absorber,
            spell,
            absorb_spell,
            amount,
            ..
        } = e
        else {
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
        let Event::Absorbed {
            src,
            dst,
            absorber,
            spell,
            absorb_spell,
            amount,
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(src.name, "Ulgrax the Devourer");
        assert_eq!(dst.name, "Thrall-Ragnaros");
        assert_eq!(
            absorber.name, "Moira-Ragnaros",
            "credit goes to the shield caster"
        );
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
        let Event::Interrupt {
            spell,
            interrupted_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.name, "Wind Shear");
        assert_eq!(interrupted_spell.name, "Digestive Acid");
    }

    #[test]
    fn parses_dispel() {
        let e = parse(&format!(
            "SPELL_DISPEL,{HEALER},{PLAYER},527,\"Purify\",0x2,468888,\"Carnivorous Contest\",0x20,DEBUFF"
        ));
        let Event::Dispel {
            spell,
            dispelled_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.name, "Purify");
        assert_eq!(dispelled_spell.name, "Carnivorous Contest");
    }

    #[test]
    fn parses_aura_applied_debuff() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{PLAYER},{BOSS},118,\"Polymorph\",0x40,DEBUFF"
        ));
        let Event::AuraApplied {
            spell, aura_type, ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.id, 118);
        assert_eq!(aura_type, AuraType::Debuff);
    }

    /// The optional trailing absorb amount must not be mistaken for the aura type.
    #[test]
    fn parses_aura_applied_buff_with_trailing_amount() {
        let e = parse(&format!(
            "SPELL_AURA_APPLIED,{HEALER},{PLAYER},17,\"Power Word: Shield\",0x2,BUFF,45000"
        ));
        let Event::AuraApplied { aura_type, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(aura_type, AuraType::Buff);
    }

    /// Real logs emit SPELL_AURA_APPLIED at 13, 14 AND 15 fields. aura_type is always
    /// idx12; trailing optionals are ignored and width is never gated on.
    #[test]
    fn aura_applied_tolerates_13_14_and_15_field_widths() {
        for tail in ["DEBUFF", "DEBUFF,45000", "DEBUFF,0,0"] {
            let e = parse(&format!(
                "SPELL_AURA_APPLIED,{PLAYER},{BOSS},118,\"Polymorph\",0x40,{tail}"
            ));
            let Event::AuraApplied {
                aura_type, spell, ..
            } = e
            else {
                panic!("{tail}: {e:?}")
            };
            assert_eq!(aura_type, AuraType::Debuff, "tail {tail:?}");
            assert_eq!(spell.id, 118);
        }
    }

    /// 36 real lines carry a nil sourceGUID with PLAYER flags set. Classifying those as
    /// players grows a phantom row on the meter.
    #[test]
    fn nil_guid_with_player_flags_is_not_a_player() {
        let e = parse(&format!(
            "SPELL_DAMAGE,0000000000000000,nil,0x514,0x80000000,{PLAYER},1249797,\"Shattered Sky\",0x20,3000,3000,-1,32,0,0,0,nil,nil,nil"
        ));
        let Event::Damage { src, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(src.flags & 0x400, 0x400, "the PLAYER flag really is set");
        assert!(!src.is_player(), "but a nil GUID is still nobody");
        assert!(!src.is_pet_or_guardian());
    }

    // ---- summon / death / environmental -----------------------------------

    #[test]
    fn parses_summon() {
        let e = parse(
            r#"SPELL_SUMMON,Player-1168-0C777D,"Gul-Ragnaros",0x511,0x0,Pet-0-4232-2662-31585-165189-0100AB,"Felhunter",0x1114,0x0,691,"Summon Felhunter",0x20"#,
        );
        let Event::Summon { owner, pet } = e else {
            panic!("{e:?}")
        };
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
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.name, "Thrall-Ragnaros");
        assert!(unit.is_player());
    }

    /// Verbatim real-log shape: 10 fields, a single trailing `0` (not 11 as docs imply).
    #[test]
    fn parses_real_unit_died_shape() {
        let e = parse(
            r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Player-5-0BC007E0,"Dawgoneefour-Proudmoore-US",0x2114,0x80000000,0"#,
        );
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.name, "Dawgoneefour-Proudmoore-US");
        assert_eq!(unit.flags, 0x2114);
    }

    // ---- real-log corrections (validator, verified against build 12.0.7) ----

    /// `0x80000000` exceeds i32::MAX and appears as the raid-flag on nearly every real
    /// line; unit flags reach five hex digits (`0x10a48`, a vehicle).
    #[test]
    fn flags_parse_as_u32_including_high_bit_and_five_digits() {
        let e = parse(
            r#"UNIT_DIED,0000000000000000,nil,0x80000000,0x80000000,Vehicle-0-3881-2913-77155-240391-0000682736,"L'ura",0x10a48,0x80000000,0"#,
        );
        let Event::Death { unit } = e else {
            panic!("{e:?}")
        };
        assert_eq!(unit.flags, 0x10a48);
        assert_eq!(unit.name, "L'ura", "apostrophes are legal in names");
    }

    /// Schools are inconsistently formatted within a single line: `0x1` hex here,
    /// bare decimal `106` there.
    #[test]
    fn school_accepts_hex_and_bare_decimal() {
        let e = parse(&format!(
            "SPELL_INTERRUPT,{PLAYER},{BOSS},57994,\"Wind Shear\",0x1,468999,\"Digestive Acid\",106"
        ));
        let Event::Interrupt {
            spell,
            interrupted_spell,
            ..
        } = e
        else {
            panic!("{e:?}")
        };
        assert_eq!(spell.school, 1, "0x-prefixed hex");
        assert_eq!(
            interrupted_spell.school, 106,
            "bare decimal on the same line"
        );
    }

    /// A player-summoned unit is not always a `Pet-` GUID — an Efflorescence totem is a
    /// `Creature-` with NPC flags. Ownership comes from the summon, not the GUID shape.
    #[test]
    fn summon_can_target_a_creature_guid() {
        let e = parse(
            r#"SPELL_SUMMON,Player-3676-0EC8A6B9,"Knothot-Area52-US",0x514,0x80000000,Creature-0-3881-2913-77155-47649-00006827C1,"Efflorescence",0xa28,0x80000000,145205,"Efflorescence",0x8"#,
        );
        let Event::Summon { owner, pet } = e else {
            panic!("{e:?}")
        };
        assert_eq!(owner.guid, "Player-3676-0EC8A6B9");
        assert!(pet.guid.starts_with("Creature-"));
        assert!(
            !pet.is_pet_or_guardian(),
            "0xa28 has neither Pet nor Guardian bit — attribution must come from the \
             summon event, not this predicate"
        );
    }

    #[test]
    fn pet_detection_uses_flag_bits_not_guid_prefix() {
        // Guardian bit set on a Creature- GUID.
        let guardian = Unit {
            guid: "Creature-0-1".into(),
            name: "Ebon Gargoyle".into(),
            flags: 0x2114,
        };
        assert!(guardian.is_pet_or_guardian());
        // Pet bit set.
        let pet = Unit {
            guid: "Pet-0-1".into(),
            name: "Felhunter".into(),
            flags: 0x1114,
        };
        assert!(pet.is_pet_or_guardian());
        // A Pet- GUID with no type bits must NOT be classified by its prefix.
        let bare = Unit {
            guid: "Pet-0-1".into(),
            name: "x".into(),
            flags: 0x0,
        };
        assert!(!bare.is_pet_or_guardian());
    }

    #[test]
    fn names_carry_realm_suffixes_apostrophes_and_non_ascii() {
        for (raw, want) in [
            (r#""Fidèle-Tichondrius-US""#, "Fidèle-Tichondrius-US"),
            (r#""Swegbert-Kil'jaeden-US""#, "Swegbert-Kil'jaeden-US"),
            (r#""L'ura""#, "L'ura"),
        ] {
            let e = parse(&format!(
                "SPELL_SUMMON,Player-1-A,{raw},0x511,0x80000000,Pet-0-1,\"P\",0x1114,0x80000000,691,\"S\",0x20"
            ));
            let Event::Summon { owner, .. } = e else {
                panic!("{e:?}")
            };
            assert_eq!(owner.name, want);
        }
    }

    /// envType sits at offset 28, AFTER the advanced block — not at 9 as the wiki says.
    #[test]
    fn parses_environmental_damage() {
        let e = parse(&format!(
            "ENVIRONMENTAL_DAMAGE,{NIL_UNIT},{PLAYER},{},Falling,4000,4000,-1,1,0,0,0,nil,nil,nil",
            adv("Player-1168-0A234B", "0000000000000000")
        ));
        let Event::Damage { src, amount, .. } = e else {
            panic!("{e:?}")
        };
        assert_eq!(
            amount, 4000,
            "envType must be skipped, not read as the amount"
        );
        assert!(!src.is_player(), "null source belongs to nobody");
    }

    // ---- unit flags -------------------------------------------------------

    #[test]
    fn unit_classification() {
        let player = Unit {
            guid: "Player-1-A".into(),
            name: "P".into(),
            flags: 0x511,
        };
        assert!(player.is_player());
        assert!(!player.is_pet_or_guardian());

        let pet = Unit {
            guid: "Pet-0-1".into(),
            name: "Felhunter".into(),
            flags: 0x1114,
        };
        assert!(pet.is_pet_or_guardian());
        assert!(!pet.is_player());

        let boss = Unit {
            guid: "Creature-0-1".into(),
            name: "B".into(),
            flags: 0xa48,
        };
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
        assert!(matches!(
            l,
            None | Some(LogLine {
                event: Event::Other,
                ..
            })
        ));
    }

    /// Whole-file smoke test against a real combat log. Skipped unless
    /// `WOWDPS_REAL_LOG` points at one, so the shared gate stays hermetic.
    ///
    /// Run: `WOWDPS_REAL_LOG=<path> cargo test real_log -- --nocapture`
    #[test]
    fn real_log_parses_without_loss() {
        let Ok(path) = std::env::var("WOWDPS_REAL_LOG") else {
            return;
        };
        let text = std::fs::read_to_string(&path).expect("readable log");

        let (mut total, mut none, mut other, mut dmg, mut heal, mut absorb) = (0, 0, 0, 0, 0, 0);
        let (mut dmg_sum, mut heal_sum) = (0u64, 0u64);
        for raw in text.lines() {
            if raw.trim().is_empty() {
                continue;
            }
            total += 1;
            match parse_line(raw) {
                None => {
                    if none < 5 {
                        eprintln!("UNPARSED: {raw}");
                    }
                    none += 1;
                }
                Some(l) => match l.event {
                    Event::Other => other += 1,
                    Event::Damage { amount, .. } => {
                        dmg += 1;
                        dmg_sum += amount;
                    }
                    Event::Heal {
                        amount, overheal, ..
                    } => {
                        heal += 1;
                        heal_sum += amount.saturating_sub(overheal);
                    }
                    Event::Absorbed { .. } => absorb += 1,
                    _ => {}
                },
            }
        }
        eprintln!(
            "lines={total} unparsed={none} other={other} damage={dmg} heal={heal} \
             absorbed={absorb} dmg_total={dmg_sum} effective_heal={heal_sum}"
        );
        assert_eq!(none, 0, "every line of a real log must parse");
        assert!(dmg > 0 && heal > 0 && absorb > 0, "expected real events");
    }

    #[test]
    fn unknown_event_is_other_not_none() {
        let e = parse(&format!(
            "SPELL_CAST_START,{PLAYER},{BOSS},133,\"Fireball\",0x4"
        ));
        assert_eq!(e, Event::Other);
        let e = parse("SOME_FUTURE_EVENT,1,2,3");
        assert_eq!(e, Event::Other);
    }
}
