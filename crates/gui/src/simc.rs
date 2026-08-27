//! A reader for the SimulationCraft addon's export paste — the talent
//! viewer's richest input. One paste is a whole machine-readable character:
//! identity, the active `talents=` string, every `# Saved Loadout:` string,
//! equipped gear, the `### Gear from Bags` section, and the commented
//! currency lines. Parsing is line-oriented and forgiving: unknown lines
//! are skipped, a malformed item line is dropped rather than an error, and
//! the only hard failure is a paste with nothing we recognize in it.
//!
//! Stdlib only, no state — the GUI keeps the raw paste and re-parses at
//! will (a paste is a few KB).

/// A talent build named by the paste: the active one, or a saved loadout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loadout {
    pub name: String,
    pub string: String,
    /// False for saved loadouts, true for the uncommented `talents=` line.
    pub active: bool,
}

/// One item line (`slot=,id=…,bonus_id=…`), equipped or bagged. The name
/// and ilvl come from the `# Name (ilvl)` comment the addon writes above
/// the line, when present.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub slot: String,
    pub id: u64,
    pub name: Option<String>,
    pub ilvl: Option<u32>,
    pub enchant_id: Option<u64>,
    pub gem_ids: Vec<u64>,
    pub bonus_ids: Vec<u64>,
}

/// A `c:<id>:<amount>` or `i:<id>:<amount>` entry from the addon's
/// `upgrade_currencies=` / `catalyst_currencies=` comments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Currency {
    /// True for `c:` (a game currency id), false for `i:` (an item id used
    /// as currency, e.g. crests stored as items).
    pub is_currency: bool,
    pub id: u64,
    pub amount: u64,
    /// From `catalyst_currencies=` rather than `upgrade_currencies=`.
    pub catalyst: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Profile {
    /// Character name from the `classtoken="Name"` opening line.
    pub name: Option<String>,
    /// The lowercase simc class token ("warlock", "demonhunter", …).
    pub class_token: Option<String>,
    pub spec: Option<String>,
    pub level: Option<u32>,
    pub server: Option<String>,
    /// The exporting client's version, from the `# WoW <ver>` comment.
    pub wow_version: Option<String>,
    /// Every talent string in the paste, the active `talents=` line first.
    pub loadouts: Vec<Loadout>,
    pub equipped: Vec<Item>,
    pub bags: Vec<Item>,
    pub currencies: Vec<Currency>,
}

const CLASS_TOKENS: [&str; 13] = [
    "warrior",
    "paladin",
    "hunter",
    "rogue",
    "priest",
    "deathknight",
    "shaman",
    "mage",
    "warlock",
    "monk",
    "druid",
    "demonhunter",
    "evoker",
];

const SLOTS: [&str; 18] = [
    "head",
    "neck",
    "shoulder",
    "back",
    "chest",
    "shirt",
    "tabard",
    "wrist",
    "hands",
    "waist",
    "legs",
    "feet",
    "finger1",
    "finger2",
    "trinket1",
    "trinket2",
    "main_hand",
    "off_hand",
];

/// Does the text look like a whole simc paste rather than a bare talent
/// import string? (One line that is pure base64 alphabet is a string.)
pub(crate) fn looks_like_profile(text: &str) -> bool {
    text.lines().count() > 1 || text.contains('=')
}

/// Parse a paste. Err only when nothing recognizable was found.
pub(crate) fn parse(text: &str) -> Result<Profile, String> {
    let mut p = Profile::default();
    // The `# Name (ilvl)` comment the addon writes above an item line.
    let mut pending_name: Option<(String, u32)> = None;
    // A `# Saved Loadout: X` comment waiting for its `# talents=` line.
    let mut pending_loadout: Option<String> = None;
    let mut in_bags = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (body, commented) = match line.strip_prefix('#') {
            Some(rest) => (rest.trim(), true),
            None => (line, false),
        };
        if body.is_empty() {
            continue;
        }

        if commented {
            // Section headers steer where item lines land.
            if body.starts_with("## Gear from Bags") || body == "# Gear from Bags" {
                in_bags = true;
                continue;
            }
            if body.starts_with("## ") {
                // Any other ### section ends the bags block (weekly rewards
                // also carry item lines; those are choices, not possessions).
                in_bags = false;
                continue;
            }
            if let Some(name) = body.strip_prefix("Saved Loadout:") {
                pending_loadout = Some(name.trim().to_string());
                continue;
            }
            if let Some(s) = body.strip_prefix("talents=") {
                let name = pending_loadout
                    .take()
                    .unwrap_or_else(|| "saved".to_string());
                push_loadout(&mut p, name, s, false);
                continue;
            }
            if let Some(v) = body.strip_prefix("WoW ") {
                // "# WoW 12.1.0.69497, TOC 120100" — keep the version word.
                let ver = v.split([',', ' ']).next().unwrap_or(v);
                if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    p.wow_version = Some(ver.to_string());
                }
                continue;
            }
            if let Some(list) = body.strip_prefix("upgrade_currencies=") {
                parse_currencies(list, false, &mut p.currencies);
                continue;
            }
            if let Some(list) = body.strip_prefix("catalyst_currencies=") {
                parse_currencies(list, true, &mut p.currencies);
                continue;
            }
            if let Some(item) = parse_item_line(body) {
                let item = named(item, &mut pending_name);
                if in_bags {
                    p.bags.push(item);
                }
                // Commented item lines outside the bags section are the
                // weekly-reward choices — offered, not owned; skipped.
                continue;
            }
            // "# Fireshroud Cowl (639)" — remember for the item line below.
            // Any other comment breaks the pairing, so a name can never
            // drift down onto some later, unrelated item line.
            pending_name = parse_name_comment(body);
            continue;
        }

        // Uncommented lines: identity, the active talents, equipped gear.
        if let Some(s) = body.strip_prefix("talents=") {
            push_loadout(&mut p, "active".to_string(), s, true);
            continue;
        }
        if let Some((key, val)) = body.split_once('=') {
            let val_unq = val.trim().trim_matches('"');
            if CLASS_TOKENS.contains(&key) {
                p.class_token = Some(key.to_string());
                p.name = Some(val_unq.to_string());
                continue;
            }
            match key {
                "spec" => p.spec = Some(val_unq.to_string()),
                "server" => p.server = Some(val_unq.to_string()),
                "level" => p.level = val_unq.parse().ok(),
                _ => {
                    if let Some(item) = parse_item_line(body) {
                        p.equipped.push(named(item, &mut pending_name));
                    }
                }
            }
        }
    }

    let empty = p.loadouts.is_empty()
        && p.equipped.is_empty()
        && p.bags.is_empty()
        && p.currencies.is_empty()
        && p.name.is_none();
    if empty {
        return Err(
            "nothing recognizable in the paste — expected a SimulationCraft addon export"
                .to_string(),
        );
    }
    Ok(p)
}

/// Attach a pending `# Name (ilvl)` comment to the item line under it.
fn named(mut item: Item, pending: &mut Option<(String, u32)>) -> Item {
    if let Some((name, ilvl)) = pending.take() {
        item.name = Some(name);
        item.ilvl = Some(ilvl);
    }
    item
}

fn push_loadout(p: &mut Profile, name: String, string: &str, active: bool) {
    let string = string.trim().to_string();
    if string.is_empty() {
        return;
    }
    // The addon exports the active build among the saved ones too; one copy
    // (the active line's) is enough.
    if p.loadouts.iter().any(|l| l.string == string) {
        return;
    }
    let slot = Loadout {
        name,
        string,
        active,
    };
    if active {
        p.loadouts.insert(0, slot);
    } else {
        p.loadouts.push(slot);
    }
}

/// "Fireshroud Cowl (639)" → (name, ilvl). Anything else is not a name
/// comment.
fn parse_name_comment(body: &str) -> Option<(String, u32)> {
    let (name, tail) = body.rsplit_once(" (")?;
    let ilvl: u32 = tail.strip_suffix(')')?.parse().ok()?;
    (!name.is_empty() && !name.contains('=')).then(|| (name.to_string(), ilvl))
}

/// "head=,id=212095,enchant_id=7328,gem_id=213743/213743,bonus_id=6652" →
/// an [`Item`]. None when the line is not slot-shaped or has no id.
fn parse_item_line(body: &str) -> Option<Item> {
    let (slot, rest) = body.split_once('=')?;
    if !SLOTS.contains(&slot) {
        return None;
    }
    let mut item = Item {
        slot: slot.to_string(),
        id: 0,
        name: None,
        ilvl: None,
        enchant_id: None,
        gem_ids: Vec::new(),
        bonus_ids: Vec::new(),
    };
    for field in rest.split(',') {
        let Some((k, v)) = field.split_once('=') else {
            continue;
        };
        let ids = || v.split('/').filter_map(|n| n.parse::<u64>().ok());
        match k {
            "id" => item.id = v.parse().ok()?,
            "enchant_id" => item.enchant_id = v.parse().ok(),
            "gem_id" => item.gem_ids = ids().collect(),
            "bonus_id" => item.bonus_ids = ids().collect(),
            _ => {}
        }
    }
    (item.id > 0).then_some(item)
}

/// "c:2915:406/i:210221:5" → currencies. Malformed entries are skipped.
fn parse_currencies(list: &str, catalyst: bool, out: &mut Vec<Currency>) {
    for entry in list.split('/') {
        let mut it = entry.split(':');
        let kind = it.next();
        let is_currency = match kind {
            Some("c") => true,
            Some("i") => false,
            _ => continue,
        };
        let (Some(id), Some(amount)) = (
            it.next().and_then(|v| v.parse().ok()),
            it.next().and_then(|v| v.parse().ok()),
        ) else {
            continue;
        };
        out.push(Currency {
            is_currency,
            id,
            amount,
            catalyst,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASTE: &str = r#"
warlock="Tranqlock"
level=80
race=orc
region=us
server=proudmoore
role=spell
professions=alchemy=100/herbalism=75
spec=demonology

talents=CoQAAAAAAAAAAAAAAAAAAAAAAAAglkQSSkkkkIJRSSaB

# Saved Loadout: Raid ST
# talents=CoQAAAAAAAAAAAAAAAAAAAAAAAAglkQSSkkkkIJRSSaB
# Saved Loadout: M+ AoE
# talents=CoQAAAAAAAAAAAAAAAAAAAAAAAAglkQSSkkkkIJRSSaC

# WoW 12.1.0.69497, TOC 120100

# Abyssal Immolator's Hood (639)
head=,id=212095,bonus_id=6652/10356,gem_id=213743,enchant_id=7328
neck=,id=252009,bonus_id=1533
main_hand=,id=222442,enchant_id=7460

# upgrade_currencies=c:2915:406/c:3008:12/i:210221:5
# catalyst_currencies=c:3116:4
# slot_high_watermarks=1:639:639/14:0:298

### Gear from Bags
#
# Slippers of Serene Descent (626)
# feet=,id=221507,bonus_id=6652
# Fyrakk's Tainted Rageheart (620)
# trinket1=,id=207174

### Weekly Reward Choices
#
# Entombed Seraph's Casque (658)
# head=,id=211942,bonus_id=41
### End of Weekly Reward Choices
"#;

    #[test]
    fn a_full_paste_parses() {
        let p = parse(PASTE).unwrap();
        assert_eq!(p.name.as_deref(), Some("Tranqlock"));
        assert_eq!(p.class_token.as_deref(), Some("warlock"));
        assert_eq!(p.spec.as_deref(), Some("demonology"));
        assert_eq!(p.level, Some(80));
        assert_eq!(p.wow_version.as_deref(), Some("12.1.0.69497"));

        // Three distinct strings: the active line (deduped against its
        // saved copy) plus the second saved loadout.
        assert_eq!(p.loadouts.len(), 2);
        let active = p.loadouts.first().unwrap();
        assert!(active.active);
        assert_eq!(active.name, "active");
        assert_eq!(p.loadouts[1].name, "M+ AoE");
        assert!(p.loadouts[1].string.ends_with('C'));

        assert_eq!(p.equipped.len(), 3);
        let head = &p.equipped[0];
        assert_eq!(head.slot, "head");
        assert_eq!(head.id, 212095);
        assert_eq!(head.name.as_deref(), Some("Abyssal Immolator's Hood"));
        assert_eq!(head.ilvl, Some(639));
        assert_eq!(head.enchant_id, Some(7328));
        assert_eq!(head.gem_ids, vec![213743]);
        assert_eq!(head.bonus_ids, vec![6652, 10356]);
        // The comment binds to the line under it only.
        assert_eq!(p.equipped[1].name, None);

        // Bag items come from the bags section; weekly reward choices do
        // not count as possessions.
        assert_eq!(p.bags.len(), 2);
        assert_eq!(p.bags[0].slot, "feet");
        assert_eq!(
            p.bags[0].name.as_deref(),
            Some("Slippers of Serene Descent")
        );
        assert_eq!(p.bags[1].id, 207174);
        assert!(!p.bags.iter().any(|i| i.id == 211942));

        let cur: Vec<_> = p.currencies.iter().filter(|c| !c.catalyst).collect();
        assert_eq!(cur.len(), 3);
        assert!(cur[0].is_currency && cur[0].id == 2915 && cur[0].amount == 406);
        assert!(!cur[2].is_currency && cur[2].id == 210221);
        let cat: Vec<_> = p.currencies.iter().filter(|c| c.catalyst).collect();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].amount, 4);
    }

    #[test]
    fn garbage_is_an_error_and_junk_lines_are_skipped() {
        assert!(parse("").is_err());
        assert!(parse("once upon a time\nthere was no gear\n").is_err());
        // A recognizable core survives surrounding junk.
        let p = parse("nonsense\nmage=\"Frosty\"\nwhat=ever\nhead=,id=abc\n").unwrap();
        assert_eq!(p.name.as_deref(), Some("Frosty"));
        assert!(p.equipped.is_empty(), "id=abc must not parse");
    }

    #[test]
    fn profile_versus_bare_string() {
        assert!(looks_like_profile(PASTE));
        assert!(!looks_like_profile(
            "CoQAAAAAAAAAAAAAAAAAAAAAAAAglkQSSkkkkIJRSSaB"
        ));
    }
}
