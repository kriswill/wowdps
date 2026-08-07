//! Parser for WoWDBDefs `.dbd` schema files.
//!
//! A `.dbd` starts with a COLUMNS section declaring every column's type
//! (`int`/`float`/`string`/`locstring`, optionally with a foreign-key
//! annotation and a trailing `?` for unverified names), followed by version
//! blocks separated by blank lines. Each block carries `LAYOUT` hashes,
//! `BUILD` ranges, and the record layout: one column per line with optional
//! `$id$`/`$relation$`/`$noninline,...$` annotations, `<size>` bit widths
//! (`u` prefix = unsigned), and `[n]` array lengths. A DB2 file is matched to
//! a block by its header's `layout_hash`.

use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColType {
    Int,
    Float,
    Str,
    /// Localized string; one CSV column in modern (post-Legion) clients.
    LocStr,
}

pub struct Dbd {
    pub types: HashMap<String, ColType>,
    pub versions: Vec<VersionDef>,
}

pub struct VersionDef {
    pub layouts: Vec<u32>,
    pub builds: Vec<String>,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub is_id: bool,
    pub is_relation: bool,
    pub noninline: bool,
    pub unsigned: bool,
    /// Bit width from `<size>`; None for floats, strings and non-inline ids.
    pub bits: Option<u32>,
    /// Array length from `[n]`; None for scalars.
    pub array: Option<u32>,
}

impl Dbd {
    pub fn parse(text: &str) -> Result<Dbd, String> {
        let mut types = HashMap::new();
        let mut versions = Vec::new();
        let mut lines = text.lines().map(|l| l.trim_end_matches('\r')).peekable();

        match lines.next() {
            Some("COLUMNS") => {}
            other => return Err(format!("dbd: expected COLUMNS, got {other:?}")),
        }
        for line in lines.by_ref() {
            if line.trim().is_empty() {
                break;
            }
            let (ty, name) = parse_column(line)?;
            types.insert(name, ty);
        }

        // Version blocks, separated by blank lines.
        let mut block: Vec<&str> = Vec::new();
        for line in lines.chain(std::iter::once("")) {
            if line.trim().is_empty() {
                if !block.is_empty() {
                    versions.push(parse_block(&block)?);
                    block.clear();
                }
            } else {
                block.push(line);
            }
        }

        Ok(Dbd { types, versions })
    }

    /// The version block whose LAYOUT list contains `hash`.
    pub fn version_for_layout(&self, hash: u32) -> Option<&VersionDef> {
        self.versions.iter().find(|v| v.layouts.contains(&hash))
    }

    pub fn known_layouts(&self) -> Vec<u32> {
        self.versions
            .iter()
            .flat_map(|v| v.layouts.iter().copied())
            .collect()
    }

    pub fn col_type(&self, name: &str) -> Result<ColType, String> {
        self.types
            .get(name)
            .copied()
            .ok_or_else(|| format!("dbd: layout references undeclared column {name}"))
    }
}

/// `int<Foreign::Col> Name? // comment` -> (type, name without `?`).
fn parse_column(line: &str) -> Result<(ColType, String), String> {
    let line = strip_comment(line);
    let (ty_part, name_part) = line
        .split_once(' ')
        .ok_or_else(|| format!("dbd: bad column line {line:?}"))?;
    let ty_name = ty_part.split('<').next().unwrap_or(ty_part);
    let ty = match ty_name {
        "int" => ColType::Int,
        "float" => ColType::Float,
        "string" => ColType::Str,
        "locstring" => ColType::LocStr,
        other => return Err(format!("dbd: unknown column type {other:?} in {line:?}")),
    };
    let name = name_part.trim().trim_end_matches('?').to_string();
    if name.is_empty() {
        return Err(format!("dbd: empty column name in {line:?}"));
    }
    Ok((ty, name))
}

fn parse_block(lines: &[&str]) -> Result<VersionDef, String> {
    let mut def = VersionDef {
        layouts: Vec::new(),
        builds: Vec::new(),
        fields: Vec::new(),
    };
    for line in lines {
        if let Some(rest) = line.strip_prefix("LAYOUT ") {
            for h in rest.split(',') {
                let h = h.trim();
                def.layouts.push(
                    u32::from_str_radix(h, 16)
                        .map_err(|_| format!("dbd: bad layout hash {h:?}"))?,
                );
            }
        } else if let Some(rest) = line.strip_prefix("BUILD ") {
            def.builds
                .extend(rest.split(',').map(|b| b.trim().to_string()));
        } else if line.starts_with("COMMENT") {
            // ignored
        } else {
            def.fields.push(parse_field(line)?);
        }
    }
    Ok(def)
}

/// `$noninline,id$Name<u32>[2] // comment`
fn parse_field(line: &str) -> Result<FieldDef, String> {
    let mut rest = strip_comment(line);
    let mut f = FieldDef {
        name: String::new(),
        is_id: false,
        is_relation: false,
        noninline: false,
        unsigned: false,
        bits: None,
        array: None,
    };

    if let Some(after) = rest.strip_prefix('$') {
        let (anns, tail) = after
            .split_once('$')
            .ok_or_else(|| format!("dbd: unterminated annotation in {line:?}"))?;
        for ann in anns.split(',') {
            match ann {
                "id" => f.is_id = true,
                "relation" => f.is_relation = true,
                "noninline" => f.noninline = true,
                other => return Err(format!("dbd: unknown annotation {other:?} in {line:?}")),
            }
        }
        rest = tail;
    }

    let (name, tail) = match rest.find(['<', '[']) {
        Some(i) => rest
            .split_at_checked(i)
            .ok_or_else(|| format!("dbd: bad field name in {line:?}"))?,
        None => (rest, ""),
    };
    f.name = name.trim().to_string();
    if f.name.is_empty() {
        return Err(format!("dbd: empty field name in {line:?}"));
    }
    rest = tail;

    if let Some(after) = rest.strip_prefix('<') {
        let (size, tail) = after
            .split_once('>')
            .ok_or_else(|| format!("dbd: unterminated <size> in {line:?}"))?;
        let size = match size.strip_prefix('u') {
            Some(s) => {
                f.unsigned = true;
                s
            }
            None => size,
        };
        f.bits = Some(
            size.parse()
                .map_err(|_| format!("dbd: bad size in {line:?}"))?,
        );
        rest = tail;
    }

    if let Some(after) = rest.strip_prefix('[') {
        let (len, tail) = after
            .split_once(']')
            .ok_or_else(|| format!("dbd: unterminated [len] in {line:?}"))?;
        f.array = Some(
            len.parse()
                .map_err(|_| format!("dbd: bad array len in {line:?}"))?,
        );
        rest = tail;
    }

    if !rest.trim().is_empty() {
        return Err(format!("dbd: trailing junk {rest:?} in {line:?}"));
    }
    Ok(f)
}

fn strip_comment(line: &str) -> &str {
    match line.split_once("//") {
        Some((before, _)) => before.trim(),
        None => line.trim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
COLUMNS
int ID
int<SkillLine::ID> SkillLine
int<Spell::ID> Spell
int RaceMasks?
locstring AbilityVerb_lang?
float Coord
string Title

BUILD 0.5.3.3368
$id$ID<32>
Spell<32>

LAYOUT F98AA48E, 0E85F43A
BUILD 12.1.0.68209, 12.1.0.68301
BUILD 11.0.0.54210-11.0.2.55000
COMMENT some note
AbilityVerb_lang
$id$ID<32>
$relation$SkillLine<16>
Spell<32>
RaceMasks<32>[2]
Coord // trailing comment
Title
";

    #[test]
    fn parses_columns_and_blocks() {
        let dbd = Dbd::parse(SAMPLE).unwrap();
        assert_eq!(dbd.col_type("SkillLine").unwrap(), ColType::Int);
        assert_eq!(dbd.col_type("AbilityVerb_lang").unwrap(), ColType::LocStr);
        assert_eq!(dbd.col_type("Coord").unwrap(), ColType::Float);
        assert_eq!(dbd.versions.len(), 2);

        let v = dbd.version_for_layout(0xF98A_A48E).unwrap();
        assert_eq!(v.layouts, vec![0xF98A_A48E, 0x0E85_F43A]);
        assert_eq!(v.builds.len(), 3);
        assert_eq!(v.fields.len(), 7);

        let id = &v.fields[1];
        assert!(id.is_id && !id.noninline);
        assert_eq!(id.bits, Some(32));

        let rel = &v.fields[2];
        assert!(rel.is_relation && !rel.noninline);
        assert_eq!(rel.bits, Some(16));

        let arr = &v.fields[4];
        assert_eq!(arr.array, Some(2));
        assert_eq!(arr.bits, Some(32));
        assert!(!arr.unsigned);

        assert_eq!(v.fields[5].bits, None); // float
        assert_eq!(v.fields[6].bits, None); // string
    }

    #[test]
    fn noninline_and_unsigned() {
        let f = parse_field("$noninline,id$ID").unwrap();
        assert!(f.is_id && f.noninline);
        assert_eq!(f.bits, None);

        let f = parse_field("Mask<u64>").unwrap();
        assert!(f.unsigned);
        assert_eq!(f.bits, Some(64));

        let f = parse_field("$noninline,relation$SpellID").unwrap();
        assert!(f.is_relation && f.noninline);
    }

    #[test]
    fn unknown_layout_is_none() {
        let dbd = Dbd::parse(SAMPLE).unwrap();
        assert!(dbd.version_for_layout(0xDEAD_BEEF).is_none());
    }
}
