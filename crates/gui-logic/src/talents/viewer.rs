//! The viewer's state: the paste or logged loadout it shows, the build as
//! an editable selection map, the clicks that edit it under the game's
//! rules (pane caps, point gates, orphan cascades), and the pointer's
//! hover and picker bookkeeping. Every GUI drives one `Viewer` through
//! [`Viewer::on_msg`] and draws its `build`.

use std::collections::HashMap;
use std::rc::Rc;

use wowdps_proto::json::Json;
use wowdps_proto::talents as codec;

use super::model::{Build, Node, Sel, build_model, decode_build};
use crate::simc::{self, load_stored, save_stored, store_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Talents,
    Inventory,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// The one-line input changed (a bare import string is typed/pasted).
    Input(String),
    /// Decode the input line.
    Submit,
    /// Read the system clipboard (how a multi-line simc export arrives —
    /// a single-line input would fold it).
    PasteClipboard,
    Clipboard(Option<String>),
    SelectLoadout(usize),
    SetTab(Tab),
    /// Tab key: flip between the talents and inventory tabs.
    ToggleTab,
    /// Left-click on a node: select / add a rank, or open the choice
    /// picker on an octagon node.
    NodeClick(u64),
    /// Right-click on a node: refund a rank / deselect.
    NodeRightClick(u64),
    /// A choice-picker option was clicked: (node id, entry index).
    PickChoice(u64, u64),
    /// A click landed outside the open picker.
    ClosePicker,
    /// The pointer entered a node (or, with an index, one of the open
    /// picker's option tiles) — the tab-wide overlay draws its tooltip
    /// (drawn per-pane it would clip at the pane's edge). Carries the
    /// tile's center in window coordinates: the tooltip anchors beside
    /// the icon instead of chasing the pointer.
    HoverSet(u64, Option<u64>, f32, f32),
    /// The pointer left the node (carries the id so a stale clear from
    /// one pane cannot cancel a fresh hover in another).
    HoverClear(u64),
    /// Encode the current (possibly edited) build and copy the string.
    CopyString,
    Close,
}

/// The talent viewer's whole state, framework-free.
pub struct Viewer {
    pub input: String,
    /// The meter row this was opened from, if any.
    pub player: Option<String>,
    pub profile: Option<simc::Profile>,
    pub loadout_sel: usize,
    pub tab: Tab,
    /// The editable source of truth: spec + selected nodes + hero pick.
    /// `build` is re-derived from it after every click.
    pub spec_id: Option<u64>,
    pub sels: HashMap<u64, Sel>,
    pub hero: Option<u64>,
    /// The spec's `tree_view` output, cached per spec id: it deep-clones
    /// every tooltip string and depends only on the spec, so per-click
    /// rebuilds must not re-run it.
    pub tree: Option<(u64, Json)>,
    /// Warnings from the last decoded string; shown until a fresh string
    /// is loaded.
    pub warnings: Vec<String>,
    pub build: Option<Build>,
    pub error: Option<String>,
    /// The choice node whose option picker is expanded.
    pub picker: Option<u64>,
    /// The node under the pointer (and the picker option's index when the
    /// pointer is on an option tile), for the tab-wide tooltip overlay.
    pub hover: Option<(u64, Option<u64>)>,
    /// The hovered tile's center, window coordinates — the tooltip's
    /// anchor.
    pub hover_at: (f32, f32),
    /// Any click changed the build since it was decoded.
    pub edited: bool,
    /// v19: the shown build came from the daemon's COMBATANT_INFO loadout —
    /// the player's actual logged picks, not a paste. Cleared the moment the
    /// user loads anything else (a paste, a simc loadout chip).
    pub logged: bool,
    /// v19: the logged equipped gear, for the inventory tab. Ids only — the
    /// log carries no item names.
    pub logged_gear: Option<Vec<wowdps_model::GearItem>>,
}

impl Viewer {
    /// A viewer showing nothing, on nobody.
    pub fn empty() -> Self {
        Self {
            input: String::new(),
            player: None,
            profile: None,
            loadout_sel: 0,
            tab: Tab::Talents,
            spec_id: None,
            sels: HashMap::new(),
            hero: None,
            tree: None,
            warnings: Vec::new(),
            build: None,
            error: None,
            picker: None,
            hover: None,
            hover_at: (0.0, 0.0),
            edited: false,
            logged: false,
            logged_gear: None,
        }
    }

    /// Open, optionally on a player from the meter: a stored simc paste
    /// wins, else the spec id from the wire draws the empty tree.
    pub fn open(player: Option<(String, Option<u32>)>) -> Self {
        let mut ui = Self::empty();
        if let Some((name, spec_id)) = player {
            ui.player = Some(name.clone());
            if let Some(text) = load_stored(&name) {
                ui.ingest(&text);
            } else if let Some(spec_id) = spec_id {
                ui.spec_id = Some(spec_id as u64);
                ui.rebuild();
            }
        }
        ui
    }

    /// Re-derive the laid-out panes from the selection state, through the
    /// per-spec `tree_view` cache.
    pub fn rebuild(&mut self) {
        let Some(spec_id) = self.spec_id else {
            return;
        };
        if self.tree.as_ref().map(|(id, _)| *id) != Some(spec_id) {
            match super::load_dataset().and_then(|ds| codec::tree_view(ds, spec_id)) {
                Ok(tv) => self.tree = Some((spec_id, tv)),
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            }
        }
        let Some((_, tv)) = &self.tree else {
            return;
        };
        match build_model(tv, &self.sels, self.hero, self.warnings.clone()) {
            Ok(b) => self.build = Some(b),
            Err(e) => self.error = Some(e),
        }
    }

    /// Install a freshly decoded string as the editing state.
    fn adopt(&mut self, string: &str) {
        match decode_build(string) {
            Ok((spec_id, sels, hero, warnings)) => {
                self.spec_id = Some(spec_id);
                self.sels = sels;
                self.hero = hero;
                self.warnings = warnings;
                self.picker = None;
                self.edited = false;
                self.rebuild();
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// v19: install the daemon's COMBATANT_INFO loadout — the build this
    /// player actually ran in the watched fight. It wins over whatever the
    /// viewer opened with (a stored simc paste stays one loadout-chip click
    /// away), and is never persisted: the daemon re-answers on every open.
    /// The picks round-trip through the real codec (`picks_to_selections` →
    /// `encode` → `adopt`), so validation, granted/hero handling and "copy
    /// string" all behave exactly as for a pasted build.
    pub fn adopt_logged(&mut self, l: &wowdps_model::Loadout) {
        let Some(spec_id) = l.spec_id.map(u64::from).or(self.spec_id) else {
            return;
        };
        let ds = match super::load_dataset() {
            Ok(ds) => ds,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let picks: Vec<(u32, u32, u32)> = l
            .talents
            .iter()
            .map(|t| (t.node_id, t.entry_id, t.rank))
            .collect();
        let converted = match codec::picks_to_selections(ds, spec_id, &picks) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let sels = match converted.get("selections") {
            Some(Json::Arr(s)) => s.clone(),
            _ => Vec::new(),
        };
        let string = match codec::encode(ds, spec_id, &sels) {
            Ok(enc) => enc.get("string").and_then(Json::as_str).map(str::to_string),
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let Some(string) = string else {
            return;
        };
        // `adopt` reports failure only through `self.error` — a failed
        // round-trip (codec drift) must not hang the "from combat log"
        // badge and logged gear over whatever build was already showing.
        self.error = None;
        self.adopt(&string);
        if self.error.is_some() {
            return;
        }
        // Conversion warnings (skipped drift picks, clamped ranks) surface
        // with the decode's own; the model is rebuilt to carry them.
        if let Some(Json::Arr(ws)) = converted.get("warnings")
            && !ws.is_empty()
        {
            let extra = ws.iter().filter_map(Json::as_str).map(str::to_string);
            self.warnings.splice(0..0, extra);
            self.rebuild();
        }
        self.logged = true;
        self.logged_gear = (!l.gear.is_empty()).then(|| l.gear.clone());
    }

    /// Forget the logged build wholesale. `logged` and `logged_gear` must
    /// move together: the inventory chip gates on the gear, the content arm
    /// on the flag, and a half-cleared pair renders one tab's chrome over
    /// the other's content. Falls back to the talents tab when the simc
    /// profile has no inventory left to show.
    fn drop_logged(&mut self) {
        self.logged = false;
        self.logged_gear = None;
        if self.tab == Tab::Inventory && !self.profile_has_inventory() {
            self.tab = Tab::Talents;
        }
    }

    /// The pasted profile carries equipped gear, bags or currencies.
    pub fn profile_has_inventory(&self) -> bool {
        self.profile.as_ref().is_some_and(|p| {
            !p.equipped.is_empty() || !p.bags.is_empty() || !p.currencies.is_empty()
        })
    }

    /// Whether the inventory tab exists: a profile's inventory, or v19's
    /// logged gear without any paste.
    pub fn has_inventory(&self) -> bool {
        self.profile_has_inventory() || self.logged_gear.is_some()
    }

    /// The current (possibly edited) build as an import string.
    pub fn encode_current(&self) -> Option<String> {
        let spec_id = self.spec_id?;
        let ds = super::load_dataset().ok()?;
        let sels: Vec<Json> = self
            .sels
            .iter()
            .map(|(id, s)| {
                let mut o = vec![
                    ("node_id".to_string(), Json::u64(*id)),
                    ("ranks".to_string(), Json::u64(s.ranks)),
                ];
                if s.granted {
                    o.push(("granted".to_string(), Json::Bool(true)));
                }
                if let Some(c) = s.choice_index {
                    o.push(("choice_index".to_string(), Json::u64(c)));
                }
                Json::Obj(o)
            })
            .collect();
        let enc = codec::encode(ds, spec_id, &sels).ok()?;
        enc.get("string").and_then(Json::as_str).map(str::to_string)
    }

    /// The node's current pane-local view, cloned out of the build.
    pub fn find_node(&self, id: u64) -> Option<Node> {
        self.build
            .as_ref()?
            .panes()
            .flat_map(|p| p.nodes.iter())
            .find(|n| n.id == id)
            .cloned()
    }

    /// Is the pane holding this node already at its point cap?
    pub fn pane_full(&self, id: u64) -> bool {
        self.build
            .iter()
            .flat_map(Build::panes)
            .find(|p| p.nodes.iter().any(|n| n.id == id))
            .is_some_and(|p| p.full())
    }

    /// Left-click: pick / add a rank; octagons expand their option picker.
    pub fn click_node(&mut self, id: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted {
            return;
        }
        if node.choice {
            if node.available || node.selected {
                self.picker = Some(id);
            }
            return;
        }
        if node.selected {
            // Another rank costs another point: the pane cap gates it.
            if node.ranks < node.max_ranks
                && !self.pane_full(id)
                && let Some(s) = self.sels.get_mut(&id)
            {
                s.ranks += 1;
                self.edited = true;
                self.rebuild();
            }
        } else if node.available {
            self.sels.insert(
                id,
                Sel {
                    ranks: 1,
                    granted: false,
                    choice_index: None,
                },
            );
            self.edited = true;
            self.rebuild();
        }
    }

    /// Right-click: refund a rank; removing a mid-tree node cascades — every
    /// node left without a taken parent goes with it.
    pub fn unclick_node(&mut self, id: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted || !node.selected {
            return;
        }
        if node.ranks > 1 {
            if let Some(s) = self.sels.get_mut(&id) {
                s.ranks -= 1;
                self.edited = true;
                self.enforce_gates();
            }
            return;
        }
        self.sels.remove(&id);
        self.cascade_orphans(id);
        self.edited = true;
        self.enforce_gates();
    }

    /// After a refund, drop any selected node whose point gate is no longer
    /// met (counting, like the game, only points spent above the gate),
    /// cascading its orphans, to a fixpoint — so an edited build can never
    /// encode into a string the game rejects. Rebuilds as it goes; the
    /// final state is laid out on return.
    fn enforce_gates(&mut self) {
        loop {
            self.rebuild();
            let Some(build) = &self.build else {
                return;
            };
            let mut broke: Option<u64> = None;
            'panes: for pane in build.panes() {
                for n in &pane.nodes {
                    if !n.selected || n.granted || n.req == 0 {
                        continue;
                    }
                    let above: u64 = pane
                        .nodes
                        .iter()
                        .filter(|m| m.selected && !m.granted && m.req < n.req)
                        .map(|m| m.ranks)
                        .sum();
                    if above < n.req {
                        broke = Some(n.id);
                        break 'panes;
                    }
                }
            }
            match broke {
                Some(id) => {
                    self.sels.remove(&id);
                    self.cascade_orphans(id);
                }
                None => return,
            }
        }
    }

    /// Drop every selected node that lost its last taken parent, to a
    /// fixpoint, within the pane the removed node lived in.
    pub fn cascade_orphans(&mut self, removed: u64) {
        let Some(build) = &self.build else {
            return;
        };
        let Some(pane) = build
            .panes()
            .find(|p| p.nodes.iter().any(|n| n.id == removed))
        else {
            return;
        };
        let pane = Rc::clone(pane);
        loop {
            let mut dropped = false;
            for (i, n) in pane.nodes.iter().enumerate() {
                if n.granted || !self.sels.contains_key(&n.id) {
                    continue;
                }
                let mut has_parent_edge = false;
                let mut fed = false;
                for &(a, b) in &pane.edges {
                    if b != i {
                        continue;
                    }
                    has_parent_edge = true;
                    if pane
                        .nodes
                        .get(a)
                        .is_some_and(|p| self.sels.contains_key(&p.id))
                    {
                        fed = true;
                        break;
                    }
                }
                if has_parent_edge && !fed {
                    self.sels.remove(&n.id);
                    dropped = true;
                }
            }
            if !dropped {
                break;
            }
        }
    }

    /// A choice-picker option was clicked.
    pub fn pick_choice(&mut self, id: u64, index: u64) {
        self.picker = None;
        let Some(node) = self.find_node(id) else {
            return;
        };
        if node.granted || index as usize >= node.options.len().max(1) {
            return;
        }
        if !(node.selected || node.available) {
            return;
        }
        self.sels.insert(
            id,
            Sel {
                ranks: 1,
                granted: false,
                choice_index: Some(index),
            },
        );
        self.edited = true;
        self.rebuild();
    }

    /// A paste arrived (clipboard or the input line): a multi-line text is
    /// a simc export, one base64 word is an import string.
    pub fn ingest(&mut self, pasted: &str) {
        let pasted = pasted.trim();
        self.error = None;
        self.picker = None;
        // The user loaded something explicitly: the logged build steps aside
        // ENTIRELY — gear included, or the inventory chip would gate on gear
        // the content arm no longer shows.
        self.drop_logged();
        if pasted.is_empty() {
            self.error = Some("the clipboard is empty".to_string());
            return;
        }
        if simc::looks_like_profile(pasted) {
            match simc::parse(pasted) {
                Ok(profile) => {
                    if let Some(name) = profile.name.as_deref() {
                        // The realm-qualified key: the paste's own
                        // name-server pair, so realms never collide.
                        let key = match profile.server.as_deref() {
                            Some(server) => format!("{name}-{server}"),
                            None => name.to_string(),
                        };
                        save_stored(&key, pasted);
                        // The viewer may have been opened on a meter row
                        // whose realm spelling differs from the paste's
                        // `server` line; save under that name too, so the
                        // row's reopen finds it. Same character only — a
                        // paste for someone else must not shadow the row's
                        // player.
                        if let Some(player) = &self.player
                            && player.split('-').next().unwrap_or(player).to_lowercase()
                                == name.to_lowercase()
                            && store_path(player) != store_path(&key)
                        {
                            save_stored(player, pasted);
                        }
                        // Adopt the paste's character as the viewed player
                        // unless the viewer was opened on someone specific.
                        if self.player.is_none() {
                            self.player = Some(key);
                        }
                    }
                    self.profile = Some(profile);
                    self.loadout_sel = 0;
                    self.decode_selected();
                }
                Err(e) => self.error = Some(e),
            }
        } else {
            self.profile = None;
            self.loadout_sel = 0;
            self.adopt(pasted);
        }
    }

    fn decode_selected(&mut self) {
        let Some(string) = self
            .profile
            .as_ref()
            .and_then(|p| p.loadouts.get(self.loadout_sel))
            .map(|l| l.string.clone())
        else {
            self.error = Some("the paste carried no talent strings".to_string());
            return;
        };
        self.adopt(&string);
    }

    /// Everything except Close, PasteClipboard and CopyString, which the
    /// host handles (one drops the screen, the others need the clipboard).
    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Input(s) => self.input = s,
            Msg::Submit => {
                let line = self.input.clone();
                self.ingest(&line);
            }
            Msg::Clipboard(Some(text)) => self.ingest(&text),
            Msg::Clipboard(None) => self.error = Some("the clipboard is empty".to_string()),
            Msg::SelectLoadout(i) => {
                self.loadout_sel = i;
                self.picker = None;
                // A simc chip click is an explicit load: it wins over the
                // logged build (gear included) until the viewer reopens.
                self.drop_logged();
                self.decode_selected();
            }
            Msg::SetTab(tab) => self.tab = tab,
            Msg::ToggleTab => {
                // Keyboard parity with the tab chips; inventory only exists
                // once a profile — or logged gear — is in.
                if self.profile.is_some() || self.logged_gear.is_some() {
                    self.tab = match self.tab {
                        Tab::Talents => Tab::Inventory,
                        Tab::Inventory => Tab::Talents,
                    };
                }
            }
            Msg::NodeClick(id) => self.click_node(id),
            Msg::NodeRightClick(id) => self.unclick_node(id),
            Msg::PickChoice(id, index) => self.pick_choice(id, index),
            Msg::ClosePicker => self.picker = None,
            Msg::HoverSet(id, option, x, y) => {
                self.hover = Some((id, option));
                self.hover_at = (x, y);
                // Hovering any other node dismisses an open picker, the
                // way the in-game popover behaves.
                if option.is_none() && self.picker.is_some() && self.picker != Some(id) {
                    self.picker = None;
                }
            }
            Msg::HoverClear(id) => {
                if self.hover.map(|(n, _)| n) == Some(id) {
                    self.hover = None;
                }
            }
            // CopyString needs the clipboard; the host handles it.
            Msg::CopyString | Msg::PasteClipboard | Msg::Close => {}
        }
    }

    /// The hovered node as its tooltip describes it — reshaped into the
    /// hovered option when the pointer is on the open picker's tile, none
    /// while a picker is open elsewhere — with its pane's "Requires" class.
    pub fn hovered(&self) -> Option<(Node, String)> {
        let b = self.build.as_ref()?;
        let (id, opt) = self.hover?;
        let (node, requires) = b.panes().find_map(|p| {
            p.nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| (n.clone(), p.requires.clone()))
        })?;
        match (self.picker, opt) {
            (Some(p), Some(i)) if p == id => {
                Some((super::geometry::option_view(&node, i as usize)?, requires))
            }
            (Some(_), _) => None,
            (None, _) => Some((node, requires)),
        }
    }
}
