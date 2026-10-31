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
//! FileDataID — read through [`Atlas`], which the replay's extractor
//! (wowdps-replay-extract) uses too: gen-portraits cuts the replay's
//! medallion rings with it, and gen-class-power the replay HUD's class
//! resources (`classpowergen`). Backgrounds are box-downscaled ×2 (1612×774 native, far
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
use crate::img::{Image, downscale_box};
use crate::table::{Tables, cell};

pub const TABLES: [&str; 4] = [
    "UiTextureAtlas",
    "UiTextureAtlasMember",
    "UiTextureAtlasElement",
    "TraitSubTree",
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
#[derive(Clone, Copy)]
struct Member {
    atlas_id: u32,
    left: usize,
    top: usize,
    w: usize,
    h: usize,
}

/// The tables an [`Atlas`] reads.
pub const ATLAS_TABLES: [&str; 3] = [
    "UiTextureAtlas",
    "UiTextureAtlasMember",
    "UiTextureAtlasElement",
];

/// The client's texture atlases: an element, by its name or its id, cut
/// out of its sheet (each sheet decoded once). What the talent art and the
/// replay's medallion rings are cropped with; and every member as its
/// sheet commits it (`committed`, `cut`), what the replay HUD's class
/// resources are cut with from a 2x sheet and its 1x twin alike, whose
/// members share one element.
pub struct Atlas<'a> {
    element_by_name: HashMap<String, u32>,
    name_by_element: HashMap<u32, String>,
    member_by_element: HashMap<u32, Member>,
    members: Vec<Committed>,
    sheets: Sheets<'a>,
}

/// A member of the atlases as its sheet commits it: its name (lowercase,
/// a 2x sheet's suffix kept), its sheet (`UiTextureAtlas` id) and that
/// sheet's canvas (`UiCanvasID`: 1 a 1x sheet, 2 its 2x twin; 0 where the
/// table has no such column), and its size.
#[derive(Clone)]
pub struct Committed {
    pub name: String,
    pub atlas: u32,
    pub canvas: u32,
    pub w: usize,
    pub h: usize,
    member: Member,
}

impl<'a> Atlas<'a> {
    pub fn new(game: &'a Game, tables: &Tables) -> Result<Self, String> {
        // Element name ↔ id.
        let elements = tables.get("UiTextureAtlasElement")?;
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
        let members = tables.get("UiTextureAtlasMember")?;
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
        let m_name = members.col("CommittedName")?;
        let mut member_by_element: HashMap<u32, Member> = HashMap::new();
        let mut committed: Vec<(String, Member)> = Vec::new();
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
                committed.push((cell(row, m_name, "member")?.to_ascii_lowercase(), m));
            }
        }

        // Atlas id → sheet FileDataID, and its canvas where the table
        // says (1 a 1x sheet, 2 its 2x twin).
        let atlases = tables.get("UiTextureAtlas")?;
        let (a_id, a_fdid) = (atlases.col("ID")?, atlases.col("FileDataID")?);
        let a_canvas = atlases.col("UiCanvasID").ok();
        let mut fdid_by_atlas: HashMap<u32, u32> = HashMap::new();
        let mut canvas_by_atlas: HashMap<u32, u32> = HashMap::new();
        for row in &atlases.rows {
            let id = parse_u32(cell(row, a_id, "atlas")?, "atlas id")?;
            let fdid = parse_u32(cell(row, a_fdid, "atlas")?, "atlas fdid")?;
            fdid_by_atlas.insert(id, fdid);
            if let Some(c) = a_canvas {
                let canvas = parse_u32(cell(row, c, "atlas")?, "atlas canvas")?;
                canvas_by_atlas.insert(id, canvas);
            }
        }
        let members = committed
            .into_iter()
            .map(|(name, m)| Committed {
                name,
                atlas: m.atlas_id,
                canvas: canvas_by_atlas.get(&m.atlas_id).copied().unwrap_or(0),
                w: m.w,
                h: m.h,
                member: m,
            })
            .collect();
        Ok(Atlas {
            element_by_name,
            name_by_element,
            member_by_element,
            members,
            sheets: Sheets {
                game,
                fdid_by_atlas,
                decoded: HashMap::new(),
            },
        })
    }

    /// An element by its name (any case); `None` when the atlases hold no
    /// pixels for it.
    pub fn named(&mut self, name: &str) -> Result<Option<Image>, String> {
        match self.element_by_name.get(&name.to_ascii_lowercase()) {
            Some(&el) => self.element(el),
            None => Ok(None),
        }
    }

    /// An element by its id; `None` when no member gives it pixels.
    pub fn element(&mut self, el: u32) -> Result<Option<Image>, String> {
        let Some(member) = self.member_by_element.get(&el) else {
            return Ok(None);
        };
        let what = self
            .name_by_element
            .get(&el)
            .cloned()
            .unwrap_or_else(|| format!("element {el}"));
        self.sheets.crop(member, &what).map(Some)
    }

    /// An element's name, for a message.
    pub fn name(&self, el: u32) -> String {
        self.name_by_element
            .get(&el)
            .cloned()
            .unwrap_or_else(|| format!("element {el}"))
    }

    /// Every member whose committed name starts with `prefix` (any case),
    /// on any sheet, in the member table's order.
    pub fn committed(&self, prefix: &str) -> Vec<Committed> {
        let prefix = prefix.to_ascii_lowercase();
        self.members
            .iter()
            .filter(|c| c.name.starts_with(&prefix))
            .cloned()
            .collect()
    }

    /// A committed member cut out of its sheet (each sheet decoded once).
    pub fn cut(&mut self, c: &Committed) -> Result<Image, String> {
        self.sheets.crop(&c.member, &c.name)
    }
}

/// Crop a member out of its (cached) decoded sheet.
struct Sheets<'a> {
    game: &'a Game,
    fdid_by_atlas: HashMap<u32, u32>,
    decoded: HashMap<u32, Image>,
}

impl Sheets<'_> {
    fn crop(&mut self, m: &Member, what: &str) -> Result<Image, String> {
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
        sheet
            .crop(m.left, m.top, m.w, m.h)
            .ok_or_else(|| format!("{what}: crop out of range"))
    }
}

pub fn generate(game: &Game, tables: &Tables) -> Result<Generated, String> {
    let mut atlas = Atlas::new(game, tables)?;
    let mut entries: Vec<(u8, u32, Image)> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    // Spec backgrounds: talents-background-<class>-<spec>, halved.
    let spec_token: HashMap<u16, &str> = SPEC_TOKEN.into_iter().collect();
    for (spec_id, class_name) in classgen::SPEC_CLASS {
        let Some(spec_name) = spec_token.get(&spec_id) else {
            missing.push(format!("spec token for spec {spec_id}"));
            continue;
        };
        let want = format!("talents-background-{}-{spec_name}", token(class_name));
        let Some(img) = atlas.named(&want)? else {
            missing.push(want);
            continue;
        };
        entries.push((KIND_BACKGROUND, u32::from(spec_id), downscale_box(&img, 2)));
    }

    // Hero medallions: straight off TraitSubTree's element id. Dev/test
    // subtrees resolve like real ones; they are dropped only when their
    // element has no pixels.
    let subtrees = tables.get("TraitSubTree")?;
    let (t_id, t_el) = (
        subtrees.col("ID")?,
        subtrees.col("UiTextureAtlasElementID")?,
    );
    for row in &subtrees.rows {
        let sub = parse_u32(cell(row, t_id, "subtree")?, "subtree id")?;
        let el = parse_u32(cell(row, t_el, "subtree")?, "subtree element")?;
        let Some(img) = atlas.element(el)? else {
            missing.push(format!("subtree {sub} medallion ({})", atlas.name(el)));
            continue;
        };
        entries.push((KIND_MEDALLION, sub, img));
    }

    // Chrome: the golden ring the game draws around the medallion.
    match atlas.named(RING_ELEMENT)? {
        Some(img) => entries.push((KIND_CHROME, CHROME_RING, img)),
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

    /// The talent art reads every table an atlas does.
    #[test]
    fn the_talent_art_reads_the_atlas_tables() {
        assert!(ATLAS_TABLES.iter().all(|t| TABLES.contains(t)));
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
