//! `talent-art.bin`: the talent UI's own artwork, cropped out of the
//! client's texture atlases — the per-spec pane background paintings and
//! each hero tree's round medallion, plus the golden ring the game frames
//! the medallion with. Another per-machine cache beside the icon bins
//! (extracted Blizzard art never lands in the repo); the GUI's talent
//! viewer reads it lazily and renders fine without it.
//!
//! Sources, all local CASC: `UiTextureAtlasElement` names the crops
//! (`talents-background-<class>-<spec>` for the panes), `TraitSubTree`
//! names each hero tree's medallion element directly
//! (`UiTextureAtlasElementID` — no name matching), `UiTextureAtlasMember`
//! holds the committed rectangle, `UiTextureAtlas` the sheet's
//! FileDataID. Backgrounds are box-downscaled ×2 (1612×774 native, far
//! more than the pane needs) to keep the cache tens of MB, like the
//! spell-icon bin.
//!
//! Layout (LE), designed for the GUI's lazy seek-per-tile reader:
//!   "WDTA" | u32 version=1 | u32 count
//!   count × (u8 kind, u32 id, u16 w, u16 h, u64 offset)   sorted (kind,id)
//!   RGBA tiles at the recorded offsets (from file start)
//! kind 0 = spec background (id = spec id), kind 1 = hero-tree medallion
//! (id = TraitSubTree id), kind 2 = chrome (id 0 = the medallion ring).

use std::collections::HashMap;

use crate::blp;
use crate::classgen;
use crate::game::Game;
use crate::table::Csv;

pub const TABLES: [(&str, u32); 4] = [
    ("UiTextureAtlas", 897470),
    ("UiTextureAtlasMember", 897532),
    ("UiTextureAtlasElement", 1989276),
    ("TraitSubTree", 5534447),
];

pub const MAGIC: &[u8; 4] = b"WDTA";

pub const KIND_BACKGROUND: u8 = 0;
pub const KIND_MEDALLION: u8 = 1;
pub const KIND_CHROME: u8 = 2;
pub const CHROME_RING: u32 = 0;

const RING_ELEMENT: &str = "talents-heroclass-ring-mainpane";

/// English spec-name tokens per spec id, matching the locale-independent
/// atlas element names (`talents-background-<class>-<spec>`). Hardcoded
/// like `classgen::SPEC_CLASS` (spec ids are build-stable): the obvious
/// source, ChrSpecialization.Name_lang, is localized — on a non-enUS
/// install its names can never match the atlas names, and every
/// background would silently land in `missing`.
const SPEC_TOKEN: [(u16, &str); 40] = [
    (71, "arms"),
    (72, "fury"),
    (73, "protection"),
    (65, "holy"),
    (66, "protection"),
    (70, "retribution"),
    (253, "beastmastery"),
    (254, "marksmanship"),
    (255, "survival"),
    (259, "assassination"),
    (260, "outlaw"),
    (261, "subtlety"),
    (256, "discipline"),
    (257, "holy"),
    (258, "shadow"),
    (250, "blood"),
    (251, "frost"),
    (252, "unholy"),
    (262, "elemental"),
    (263, "enhancement"),
    (264, "restoration"),
    (62, "arcane"),
    (63, "fire"),
    (64, "frost"),
    (265, "affliction"),
    (266, "demonology"),
    (267, "destruction"),
    (268, "brewmaster"),
    (269, "windwalker"),
    (270, "mistweaver"),
    (102, "balance"),
    (103, "feral"),
    (104, "guardian"),
    (105, "restoration"),
    (577, "havoc"),
    (581, "vengeance"),
    (1480, "devourer"),
    (1467, "devastation"),
    (1468, "preservation"),
    (1473, "augmentation"),
];

pub struct Generated {
    pub bytes: Vec<u8>,
    pub backgrounds: usize,
    pub medallions: usize,
    /// Element names we looked for and could not resolve to pixels.
    pub missing: Vec<String>,
}

fn cell<'a>(row: &'a [String], c: usize, what: &str) -> Result<&'a str, String> {
    row.get(c)
        .map(String::as_str)
        .ok_or_else(|| format!("{what}: short row"))
}

fn parse_u32(s: &str, what: &str) -> Result<u32, String> {
    s.parse().map_err(|_| format!("{what}: bad number {s:?}"))
}

/// "Beast Mastery" / "DeathKnight" → the atlas element token: lowercase,
/// letters and digits only.
fn token(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// One committed atlas rectangle.
struct Member {
    atlas_id: u32,
    left: usize,
    top: usize,
    w: usize,
    h: usize,
}

/// Crop a member out of its (cached) decoded sheet.
struct Sheets<'a> {
    game: &'a Game,
    fdid_by_atlas: HashMap<u32, u32>,
    decoded: HashMap<u32, blp::Image>,
}

impl Sheets<'_> {
    fn crop(&mut self, m: &Member, what: &str) -> Result<blp::Image, String> {
        let fdid = *self
            .fdid_by_atlas
            .get(&m.atlas_id)
            .ok_or_else(|| format!("{what}: atlas {} not in UiTextureAtlas", m.atlas_id))?;
        if !self.decoded.contains_key(&m.atlas_id) {
            let data = self.game.fetch_fdid(fdid, 0x2)?;
            let img = blp::decode(&data).map_err(|e| format!("{what}: atlas {fdid}: {e}"))?;
            self.decoded.insert(m.atlas_id, img);
        }
        let sheet = self
            .decoded
            .get(&m.atlas_id)
            .ok_or_else(|| format!("{what}: sheet vanished"))?;
        if m.left + m.w > sheet.width || m.top + m.h > sheet.height {
            return Err(format!(
                "{what}: rect {}x{}+{}+{} outside {}x{} sheet",
                m.w, m.h, m.left, m.top, sheet.width, sheet.height
            ));
        }
        let mut rgba = Vec::with_capacity(m.w * m.h * 4);
        for row in 0..m.h {
            let start = ((m.top + row) * sheet.width + m.left) * 4;
            let line = sheet
                .rgba
                .get(start..start + m.w * 4)
                .ok_or_else(|| format!("{what}: crop out of range"))?;
            rgba.extend_from_slice(line);
        }
        Ok(blp::Image {
            width: m.w,
            height: m.h,
            rgba,
        })
    }
}

pub fn generate(game: &Game, tables: &HashMap<&str, Csv>) -> Result<Generated, String> {
    let get = |name: &str| -> Result<&Csv, String> {
        tables
            .get(name)
            .ok_or_else(|| format!("missing table {name}"))
    };

    // Element name ↔ id.
    let elements = get("UiTextureAtlasElement")?;
    let (e_name, e_id) = (elements.col("Name")?, elements.col("ID")?);
    let mut element_by_name: HashMap<String, u32> = HashMap::new();
    let mut name_by_element: HashMap<u32, String> = HashMap::new();
    for row in &elements.rows {
        let name = cell(row, e_name, "element")?.to_ascii_lowercase();
        let id = parse_u32(cell(row, e_id, "element")?, "element id")?;
        element_by_name.insert(name.clone(), id);
        name_by_element.insert(id, name);
    }

    // Element id → committed rectangle. Several members can share an
    // element (canvas variants); first wins, matching the client's lookup.
    let members = get("UiTextureAtlasMember")?;
    let (m_el, m_atlas) = (
        members.col("UiTextureAtlasElementID")?,
        members.col("UiTextureAtlasID")?,
    );
    let (m_w, m_h, m_left, m_top) = (
        members.col("Width")?,
        members.col("Height")?,
        members.col("CommittedLeft")?,
        members.col("CommittedTop")?,
    );
    let mut member_by_element: HashMap<u32, Member> = HashMap::new();
    for row in &members.rows {
        let el = parse_u32(cell(row, m_el, "member")?, "member element")?;
        let m = Member {
            atlas_id: parse_u32(cell(row, m_atlas, "member")?, "member atlas")?,
            left: parse_u32(cell(row, m_left, "member")?, "member left")? as usize,
            top: parse_u32(cell(row, m_top, "member")?, "member top")? as usize,
            w: parse_u32(cell(row, m_w, "member")?, "member width")? as usize,
            h: parse_u32(cell(row, m_h, "member")?, "member height")? as usize,
        };
        if m.w > 0 && m.h > 0 {
            member_by_element.entry(el).or_insert(m);
        }
    }

    // Atlas id → sheet FileDataID.
    let atlases = get("UiTextureAtlas")?;
    let (a_id, a_fdid) = (atlases.col("ID")?, atlases.col("FileDataID")?);
    let mut fdid_by_atlas: HashMap<u32, u32> = HashMap::new();
    for row in &atlases.rows {
        fdid_by_atlas.insert(
            parse_u32(cell(row, a_id, "atlas")?, "atlas id")?,
            parse_u32(cell(row, a_fdid, "atlas")?, "atlas fdid")?,
        );
    }
    let mut sheets = Sheets {
        game,
        fdid_by_atlas,
        decoded: HashMap::new(),
    };

    let mut entries: Vec<(u8, u32, blp::Image)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    // Spec backgrounds: talents-background-<class>-<spec>, halved.
    let spec_token: HashMap<u16, &str> = SPEC_TOKEN.into_iter().collect();
    for (spec_id, class_name) in classgen::SPEC_CLASS {
        let Some(spec_name) = spec_token.get(&spec_id) else {
            missing.push(format!("spec token for spec {spec_id}"));
            continue;
        };
        let want = format!("talents-background-{}-{spec_name}", token(class_name));
        let Some(member) = element_by_name
            .get(&want)
            .and_then(|el| member_by_element.get(el))
        else {
            missing.push(want);
            continue;
        };
        let img = sheets.crop(member, &want)?;
        entries.push((KIND_BACKGROUND, u32::from(spec_id), blp::downscale(&img, 2)));
    }

    // Hero medallions: straight off TraitSubTree's element id. Dev/test
    // subtrees resolve like real ones; they are dropped only when their
    // element has no pixels.
    let subtrees = get("TraitSubTree")?;
    let (t_id, t_el) = (
        subtrees.col("ID")?,
        subtrees.col("UiTextureAtlasElementID")?,
    );
    for row in &subtrees.rows {
        let sub = parse_u32(cell(row, t_id, "subtree")?, "subtree id")?;
        let el = parse_u32(cell(row, t_el, "subtree")?, "subtree element")?;
        let Some(member) = member_by_element.get(&el) else {
            let name = name_by_element
                .get(&el)
                .cloned()
                .unwrap_or_else(|| format!("element {el}"));
            missing.push(format!("subtree {sub} medallion ({name})"));
            continue;
        };
        entries.push((KIND_MEDALLION, sub, sheets.crop(member, "medallion")?));
    }

    // Chrome: the golden ring the game draws around the medallion.
    match element_by_name
        .get(RING_ELEMENT)
        .and_then(|el| member_by_element.get(el))
    {
        Some(member) => {
            entries.push((KIND_CHROME, CHROME_RING, sheets.crop(member, RING_ELEMENT)?))
        }
        None => missing.push(RING_ELEMENT.to_string()),
    }

    entries.sort_by_key(|(kind, id, _)| (*kind, *id));

    let index_at = 12usize;
    let mut offset = index_at + entries.len() * 17;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (kind, id, img) in &entries {
        bytes.push(*kind);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&(img.width as u16).to_le_bytes());
        bytes.extend_from_slice(&(img.height as u16).to_le_bytes());
        bytes.extend_from_slice(&(offset as u64).to_le_bytes());
        offset += img.rgba.len();
    }
    for (_, _, img) in &entries {
        bytes.extend_from_slice(&img.rgba);
    }

    let backgrounds = entries
        .iter()
        .filter(|(k, _, _)| *k == KIND_BACKGROUND)
        .count();
    let medallions = entries
        .iter()
        .filter(|(k, _, _)| *k == KIND_MEDALLION)
        .count();
    Ok(Generated {
        bytes,
        backgrounds,
        medallions,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header constants the GUI reader hard-codes.
    #[test]
    fn header_contract() {
        assert_eq!(MAGIC, b"WDTA");
        assert_eq!(
            (KIND_BACKGROUND, KIND_MEDALLION, KIND_CHROME, CHROME_RING),
            (0, 1, 2, 0)
        );
    }

    #[test]
    fn tokens_match_the_atlas_naming() {
        assert_eq!(token("Beast Mastery"), "beastmastery");
        assert_eq!(token("DeathKnight"), "deathknight");
        assert_eq!(token("Demonology"), "demonology");
    }

    /// Every spec the class table drives has a background token, spelled
    /// in the atlas alphabet (lowercase alphanumerics only).
    #[test]
    fn every_spec_has_a_wellformed_token() {
        let tokens: HashMap<u16, &str> = SPEC_TOKEN.into_iter().collect();
        for (spec_id, _) in classgen::SPEC_CLASS {
            let t = tokens
                .get(&spec_id)
                .unwrap_or_else(|| panic!("spec {spec_id} has no token"));
            assert!(
                !t.is_empty()
                    && t.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
                "spec {spec_id}: bad token {t:?}"
            );
        }
        assert_eq!(SPEC_TOKEN.len(), classgen::SPEC_CLASS.len());
    }
}
