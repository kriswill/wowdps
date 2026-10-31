//! What a resolved encounter or dungeon level says that cannot hold: a
//! name it uses but does not have, a size that is no size, a difficulty
//! that is none, a way to a place the map lacks.

use std::collections::BTreeMap;

use crate::schema::{Gated, hex_rgb};
use crate::{
    Aim, Breaks, Datum, Encounter, Ends, Follows, HolesBy, Instance, Level, Line, Mode, Npc, Shape,
    StencilShape, Toward, Travel, Trigger, difficulty, tint,
};

/// What an encounter names that it does not have (an ability's NPC, a
/// pet's owner, a trigger's NPC, phase or event, a phase an NPC is aloft
/// in, a color the rubric does not name), and what it says that cannot be
/// (a shape that is none, a time under 0, an `only` naming no difficulty,
/// a creature two NPCs claim, a swirl with no circle to fill or a color
/// not `#rrggbb`). Empty when it is whole. An instance's trash
/// is `check_trash`'s.
pub fn check(e: &Encounter) -> Vec<String> {
    checked(e, "encounter", false)
}

/// What an instance's trash (as `Rubric::trash` resolves it) says that
/// cannot hold: what `check` finds in an encounter, named as the trash's,
/// and a trigger counting from its start (`"start"`, an `after` with no
/// `since`), which would be the key's start. Empty when it is whole.
pub fn check_trash(e: &Encounter) -> Vec<String> {
    checked(e, "trash", true)
}

/// An encounter or a trash checked whole, `whole` naming it.
fn checked(e: &Encounter, whole: &'static str, trash: bool) -> Vec<String> {
    let mut c = Check {
        e,
        whole,
        trash,
        out: Vec::new(),
    };
    c.travel(&e.travel);
    c.npcs();
    c.abilities();
    c.phases();
    c.places();
    c.grounds();
    c.lines();
    c.volleys();
    c.knocks();
    c.ranges();
    c.aparts();
    c.streaks();
    c.rooms();
    c.reaches();
    c.events();
    c.map();
    c.only();
    c.tints();
    c.out
}

/// What a dungeon level says that cannot be: its map's, as an encounter's
/// is checked (a layer or a glow naming a phase or an event is one the
/// level does not have), a map naming another UiMap than the level's, its
/// speeds, and its pulls (`pulls`), whose members are held to the
/// instance's creatures `Rubric::level` laid into it (`Level::npc`).
/// Empty when it is whole. A level read from text alone has none laid
/// in: check it with `check_level_with`.
pub fn check_level(l: &Level) -> Vec<String> {
    check_level_with(l, &l.npc)
}

/// `check_level` with the instance's creatures given (`Instance::npcs`)
/// rather than read from the level: for a level read from text alone, as
/// an editor reads one, whose `npc` is empty, so `check_level` would find
/// every pull's members missing.
pub fn check_level_with(l: &Level, creatures: &BTreeMap<String, Npc>) -> Vec<String> {
    // The level's map as an encounter's with nothing else in it, so a
    // name it uses is found missing.
    let e = Encounter {
        map: l.map.clone(),
        view: l.view.clone(),
        travel: l.travel.clone(),
        ..Encounter::default()
    };
    let mut c = Check {
        e: &e,
        whole: "level",
        trash: false,
        out: Vec::new(),
    };
    if let Some(m) = l.map.ui_map
        && m != l.ui_map
    {
        c.out.push(format!(
            "map.ui_map names UiMap {m}, not the level's own {}",
            l.ui_map
        ));
    }
    c.travel(&l.travel);
    c.map();
    pulls(l, creatures, &mut c.out);
    c.out
}

/// A level's pulls: each in an area of its map that is no boss's court (a
/// boss is fought, not pulled), of members the instance's `creatures` and
/// at least one, a spot a member where it gives spots, each spot a number,
/// and a place in the route, from 1, no other pull has.
fn pulls(l: &Level, creatures: &BTreeMap<String, Npc>, out: &mut Vec<String>) {
    // Which pull has each place in the route, the first by key.
    let mut route: BTreeMap<u32, &str> = BTreeMap::new();
    for (k, p) in &l.pull {
        let what = format!("pull.{k}");
        match l.map.area.get(&p.area) {
            None => out.push(format!(
                "{what} stands in area {:?}, which the map does not have",
                p.area
            )),
            Some(a) => {
                if let Some(id) = a.encounter {
                    out.push(format!(
                        "{what} stands in area {:?}, encounter {id}'s court: a boss is no pull",
                        p.area
                    ));
                }
            }
        }
        if p.npc.is_empty() {
            out.push(format!("{what} pulls no one"));
        }
        let mut unknown: Vec<&str> = p
            .npc
            .iter()
            .map(String::as_str)
            .filter(|n| !creatures.contains_key(*n))
            .collect();
        unknown.sort_unstable();
        unknown.dedup();
        for n in unknown {
            out.push(format!(
                "{what} names npc {n:?}, which the instance does not have"
            ));
        }
        if !p.spots.is_empty() && p.spots.len() != p.npc.len() {
            out.push(format!(
                "{what}'s spots are {} and its members {}: a spot a member, or none",
                p.spots.len(),
                p.npc.len()
            ));
        }
        if p.spots.iter().flatten().any(|v| !v.is_finite()) {
            out.push(format!("{what} has a spot that is not a number"));
        }
        if p.order == 0 {
            out.push(format!(
                "{what} is pull 0 of the route, which counts from 1"
            ));
            continue;
        }
        match route.get(&p.order) {
            Some(other) => out.push(format!(
                "{what} is pull {} of the route, as pull.{other} is",
                p.order
            )),
            None => {
                route.insert(p.order, k);
            }
        }
    }
}

/// What an instance's own creatures (its file's `[npc]`) say that cannot
/// hold, as an encounter's are checked: two keys claiming one creature id,
/// an owner or a shared health naming a key the table lacks, a phase it is
/// aloft in (an instance has none), forces with no creature to count them
/// by, an `only` naming no difficulty; and creatures giving forces where
/// the instance gives no total. Empty when it is whole.
pub fn check_instance(i: &Instance) -> Vec<String> {
    // Its creatures as an encounter's with nothing else in it, so a name
    // they use is found missing.
    let e = Encounter {
        npc: i.npcs().clone(),
        ..Encounter::default()
    };
    let mut c = Check {
        e: &e,
        whole: "instance",
        trash: false,
        out: Vec::new(),
    };
    c.npcs();
    c.only();
    if i.forces().is_none() && i.npcs().values().any(|n| n.forces.is_some()) {
        c.out.push(
            "its creatures give enemy forces, but the instance gives no total (`forces`)".into(),
        );
    }
    c.out
}

/// The kinds of entry another names.
#[derive(Clone, Copy)]
enum Kind {
    Npc,
    Ability,
    Phase,
    Place,
    Event,
    Ground,
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Npc => "npc",
            Kind::Ability => "ability",
            Kind::Phase => "phase",
            Kind::Place => "place",
            Kind::Event => "event",
            Kind::Ground => "ground",
        }
    }
}

struct Check<'a> {
    e: &'a Encounter,
    /// What is checked: "encounter", "trash", "level" or "instance".
    whole: &'static str,
    /// An instance's trash: no trigger counts from its start.
    trash: bool,
    out: Vec<String>,
}

/// Whether a number is finite and above 0.
fn positive(v: f32) -> bool {
    v.is_finite() && v > 0.0
}

impl Check<'_> {
    fn has(&self, kind: Kind, name: &str) -> bool {
        let e = self.e;
        match kind {
            Kind::Npc => e.npc.contains_key(name),
            Kind::Ability => e.ability.contains_key(name),
            Kind::Phase => e.phase.contains_key(name),
            Kind::Place => e.place.contains_key(name),
            Kind::Event => e.event.contains_key(name),
            Kind::Ground => e.ground.contains_key(name),
        }
    }

    /// `what` names a `kind` of entry, saying `how` ("names", "is under",
    /// "breaks on" …): a problem when the encounter has none by that name.
    fn names(&mut self, what: &str, how: &str, kind: Kind, name: &str) {
        if !self.has(kind, name) {
            self.out.push(format!(
                "{what} {how} {} {name:?}, which the {} does not have",
                kind.word(),
                self.whole
            ));
        }
    }

    fn npc(&mut self, what: &str, name: &str) {
        self.names(what, "names", Kind::Npc, name);
    }

    /// `what` names a middle by `how` ("circles", "throws from …"): a
    /// problem when the encounter has neither a place nor an NPC by that
    /// name.
    fn place_or_npc(&mut self, what: &str, how: &str, name: &str) {
        if !self.has(Kind::Place, name) && !self.has(Kind::Npc, name) {
            self.out.push(format!(
                "{what} {how} {name:?}, which is neither a place nor an NPC"
            ));
        }
    }

    /// A trigger's NPC, phase or event.
    fn trigger(&mut self, what: &str, t: &Trigger) {
        match t {
            Trigger::Cast { by: Some(n), .. }
            | Trigger::CastStart { by: Some(n), .. }
            | Trigger::AuraApplied { on: Some(n), .. }
            | Trigger::AuraRemoved { on: Some(n), .. }
            | Trigger::HealthBelow { npc: n, .. }
            | Trigger::Plateau { npc: n, .. }
            | Trigger::Appears { npc: n }
            | Trigger::Death { npc: n } => self.npc(what, n),
            Trigger::Event(ev) => self.names(what, "names", Kind::Event, ev),
            Trigger::Phase(p) => self.names(what, "names", Kind::Phase, p),
            Trigger::Start if self.trash => self.out.push(format!(
                "{what} fires at the start, which in a trash file counts from the key's start"
            )),
            Trigger::After(c) => {
                if self.trash && c.since.is_none() {
                    self.out.push(format!(
                        "{what} strikes {} s after the start, which in a trash file counts from the key's start; count from a trigger (`since`)",
                        c.secs
                    ));
                }
                if !c.secs.is_finite() || c.secs < 0.0 {
                    self.out.push(format!(
                        "{what} strikes {} s after it starts counting: a clock is seconds, 0 or more",
                        c.secs
                    ));
                }
                if let Some(since) = &c.since {
                    if c.nth == Some(0) {
                        self.out.push(format!(
                            "{what} counts from the 0th time its `since` fired; `nth` counts from 1"
                        ));
                    }
                    if c.each && c.nth.is_some() {
                        self.out.push(format!(
                            "{what} counts from each time its `since` fired and from its `nth`; it does one"
                        ));
                    }
                    self.trigger(what, since);
                }
            }
            Trigger::Start
            | Trigger::Cast { by: None, .. }
            | Trigger::CastStart { by: None, .. }
            | Trigger::AuraApplied { on: None, .. }
            | Trigger::AuraRemoved { on: None, .. }
            | Trigger::Hit { .. }
            | Trigger::AuraGone { .. } => {}
        }
    }

    /// A stencil's or a glow's shape: a polygon or a circle, in numbers.
    fn shape(&mut self, what: String, s: &StencilShape) {
        let finite = s
            .points
            .iter()
            .flatten()
            .chain(s.center.iter().flatten())
            .chain(&s.radius)
            .all(|v| v.is_finite());
        if s.bounds().is_none() || !finite {
            self.out.push(format!(
                "{what} is neither a polygon (`points`, three or more) nor a circle (`center` and a `radius` above 0)"
            ));
        }
    }

    fn npcs(&mut self) {
        // Which NPC claims each creature id, the first by key.
        let mut claimed: BTreeMap<u32, &str> = BTreeMap::new();
        for (k, n) in &self.e.npc {
            let what = format!("npc.{k}");
            for o in n.owner.iter().chain(&n.shares) {
                self.npc(&what, o);
            }
            for p in &n.aloft {
                self.names(&what, "is aloft in", Kind::Phase, p);
            }
            for &c in &n.creature {
                match claimed.get(&c) {
                    Some(other) => self.out.push(format!(
                        "{what} is creature {c}, which npc.{other} is too: a unit is one NPC"
                    )),
                    None => {
                        claimed.insert(c, k);
                    }
                }
            }
            if n.forces.is_some() && n.creature.is_empty() {
                self.out.push(format!(
                    "{what} gives enemy forces but names no creature to count them by"
                ));
            }
        }
    }

    fn abilities(&mut self) {
        for (k, a) in &self.e.ability {
            let what = format!("ability.{k}");
            if let Some(n) = &a.by {
                self.npc(&what, n);
            }
            if let Some(p) = &a.phase {
                self.names(&what, "names", Kind::Phase, p);
            }
            if let Some(u) = &a.under {
                self.names(&what, "is under", Kind::Ability, u);
            }
        }
    }

    fn phases(&mut self) {
        for (k, p) in &self.e.phase {
            let what = format!("phase.{k}");
            for t in p.enter.iter().chain(&p.leave) {
                self.trigger(&what, t);
            }
            if !p.leave.is_empty() && !p.again {
                self.out
                    .push(format!("{what} has a `leave` but does not come `again`"));
            }
        }
    }

    fn places(&mut self) {
        for (k, p) in &self.e.place {
            let what = format!("place.{k}");
            for t in p.from.iter().chain(&p.until) {
                self.trigger(&what, t);
            }
            if let Some(s) = &p.swirl {
                for c in &s.colors {
                    if hex_rgb(c).is_none() {
                        self.out
                            .push(format!("{what}: swirl color {c:?} is not #rrggbb"));
                    }
                }
                if p.radius.is_none() {
                    self.out.push(format!(
                        "{what} has a swirl but no radius: it has no circle to fill"
                    ));
                }
                if !s.twist.is_finite() || !s.spin.is_finite() {
                    self.out
                        .push(format!("{what}: swirl twist or spin is not a number"));
                }
                if s.texture == 0 {
                    self.out
                        .push(format!("{what}: swirl texture 0, which is none"));
                }
            }
        }
    }

    fn grounds(&mut self) {
        for (k, g) in &self.e.ground {
            let what = format!("ground.{k}");
            if let Some(o) = &g.orbit {
                self.place_or_npc(&what, "circles", &o.around);
            }
            if let Some(p) = &g.push {
                for t in &p.on {
                    self.trigger(&what, t);
                }
                if p.on.is_empty() {
                    self.out
                        .push(format!("{what} is pushed on nothing (`on` is empty)"));
                }
                if let Toward::Bearings(b) = &p.toward
                    && (b.is_empty() || b.iter().any(|d| !d.is_finite()))
                {
                    self.out.push(format!(
                        "{what} is pushed along no bearing, or one that is not a number"
                    ));
                }
                if !positive(p.speed) || !positive(p.secs) {
                    self.out.push(format!(
                        "{what} is pushed at a speed or for a time not above 0"
                    ));
                }
                if g.orbit.is_some() {
                    self.out
                        .push(format!("{what} both circles and is pushed; it does one"));
                }
            }
            // A ground that follows turns a line or a cone to its unit
            // each moment about its spot: a circle has no way to point, an
            // orbit turns it already, a spot `ahead` of its unit is not the
            // unit it turns about, and its target is the unit a cast names,
            // never the one it lands on.
            if let Some(f) = &g.follows {
                if matches!(g.shape, Shape::Circle { .. } | Shape::Ring { .. }) {
                    self.out.push(format!(
                        "{what} follows, but a circle has no way to point: `follows` turns a line or a cone"
                    ));
                }
                if g.orbit.is_some() {
                    self.out
                        .push(format!("{what} both follows and circles; it turns one way"));
                }
                if g.ahead.is_some() {
                    self.out.push(format!(
                        "{what} both follows and lands ahead of its unit; it would turn about the spot ahead, not the unit"
                    ));
                }
                match f {
                    Follows::Target => {
                        for t in g.at.iter().filter(|t| t.marks_a_spot()) {
                            if !t.names_a_target() {
                                self.out.push(format!(
                                    "{what} follows its target, but {t:?} names none apart from the unit it lands on, which it would follow from under it: only a cast names a target (`{{ aura = N }}` follows a player carrying an aura)"
                                ));
                            }
                        }
                    }
                    Follows::Aura(0) => self.out.push(format!(
                        "{what} follows the carrier of spell 0, which is none"
                    )),
                    Follows::Aura(_) | Follows::Facing => {}
                }
            }
            // A ground falls off the stencil only where it moves, and only
            // where there is a stencil: the encounter's map's (a trash
            // file's has none, so its grounds never fall).
            if g.falls {
                if g.push.is_none() && g.orbit.is_none() {
                    self.out.push(format!(
                        "{what} falls, but nothing moves it (no `push`, no `orbit`), so it never leaves where it formed"
                    ));
                }
                if self.e.map.stencil.is_empty() {
                    self.out.push(format!(
                        "{what} falls, but the {}'s map has no stencil to fall off",
                        self.whole
                    ));
                }
            }
            // A renewal reads the firings in time order, each against the
            // grounds standing then, where their pushes have taken them: a
            // `from` stands one before the firing that places it; an `"at"`
            // ending leaves none standing; and a hit that both renews one
            // and finds it again drags it to each victim, so it follows the
            // players.
            if g.renew {
                if g.from.is_some() {
                    self.out.push(format!(
                        "{what} both renews and stands `from` a trigger; it does one"
                    ));
                }
                if g.ends == Ends::At {
                    self.out.push(format!(
                        "{what} renews but ends as `at` fires, so none stands to renew"
                    ));
                }
                for t in &g.at {
                    if let Trigger::Hit { spell, .. } = t
                        && g.seen.contains(spell)
                    {
                        self.out.push(format!(
                            "{what} renews on hits of spell {spell} that its `seen` also finds it by, so it would follow its victims; it does one"
                        ));
                    }
                }
            }
            for t in g.at.iter().chain(&g.from) {
                self.trigger(&what, t);
            }
            for t in &g.at {
                if !t.marks_a_spot() {
                    self.out.push(format!(
                        "{what} lands at {t:?}, which names no unit to stand on"
                    ));
                }
            }
            if g.at.is_empty() {
                self.out.push(format!("{what} lands at nothing"));
            }
            if g.ahead.is_some_and(|a| !a.is_finite()) {
                self.out.push(format!(
                    "{what} lands ahead by a distance that is not a number"
                ));
            }
            if g.ends.hits().iter().any(|&(spell, _)| spell == 0) {
                self.out
                    .push(format!("{what} ends at a hit of spell 0, which is none"));
            }
            // The first of some endings: hits and times, each its own.
            if let Ends::First(of) = &g.ends {
                if of.is_empty() {
                    self.out
                        .push(format!("{what} ends at the first of nothing"));
                }
                for e in of {
                    match e {
                        Ends::First(_) => self.out.push(format!(
                            "{what} ends at the first of a `first`; give its endings in one list"
                        )),
                        Ends::At | Ends::Pull => self.out.push(format!(
                            "{what} ends at the first of {:?}, which is no ending to race: give a `hit` or an `after`",
                            if *e == Ends::At { "at" } else { "pull" }
                        )),
                        Ends::Hit { .. } | Ends::After(_) => {}
                    }
                }
            }
            // What it clears as it forms, and sweeps while it stands: other
            // grounds alone. A ground this difficulty does not have left
            // them as it resolved, so a name no ground answers here is none
            // on any difficulty.
            for (how, list) in [("clears", &g.clears), ("sweeps", &g.sweeps)] {
                for c in list {
                    match self.e.ground.get(c) {
                        _ if c == k => self.out.push(format!("{what} {how} itself")),
                        Some(_) => {}
                        None if self.has(Kind::Npc, c) => self.out.push(format!(
                            "{what} {how} {c:?}, an NPC: it {how} grounds alone (a volley's `destroys` takes NPCs)"
                        )),
                        None => self.names(&what, how, Kind::Ground, c),
                    }
                }
            }
            // Each ground is read after those that clear or sweep it, so
            // none may clear one that clears it back, by any way round.
            if let Some(round) = self.clears_back(k) {
                self.out.push(format!(
                    "{what} clears or sweeps a ground that clears or sweeps it in turn ({}): each is read after those that clear it",
                    round.join(" → ")
                ));
            }
        }
    }

    /// The way round from ground `k` back to itself through the grounds
    /// each clears or sweeps (others, by the encounter's keys), as their
    /// keys from `k` to `k`, where there is one.
    fn clears_back(&self, k: &str) -> Option<Vec<String>> {
        let next = |key: &str| -> Vec<&str> {
            let Some(g) = self.e.ground.get(key) else {
                return Vec::new();
            };
            (g.clears.iter().chain(&g.sweeps))
                .map(String::as_str)
                .filter(|c| *c != key && self.e.ground.contains_key(*c))
                .collect()
        };
        // Breadth first from k, each ground's way back to k kept.
        let mut from: BTreeMap<&str, &str> = BTreeMap::new();
        let mut todo = std::collections::VecDeque::from([k]);
        while let Some(at) = todo.pop_front() {
            for n in next(at) {
                if n == k {
                    // From `at` back up to k's own: the way there, reversed.
                    let mut path = vec![at];
                    let mut back = at;
                    while let Some(&up) = from.get(back) {
                        if up == k {
                            break;
                        }
                        path.push(up);
                        back = up;
                    }
                    let mut way = vec![k.to_string()];
                    way.extend(path.into_iter().rev().map(str::to_string));
                    way.push(k.to_string());
                    return Some(way);
                }
                if !from.contains_key(n) {
                    from.insert(n, at);
                    todo.push_back(n);
                }
            }
        }
        None
    }

    fn lines(&mut self) {
        for (k, l) in &self.e.line {
            let what = format!("line.{k}");
            self.trigger(&what, &l.from);
            // `"@source"` runs to whoever put the trigger's aura on, which
            // only an aura's trigger names; any other `@` name is no
            // keyword, never an NPC's key taken on trust.
            if l.runs_to_source() {
                if !matches!(
                    l.from,
                    Trigger::AuraApplied { .. } | Trigger::AuraRemoved { .. }
                ) {
                    self.out.push(format!(
                        "{what} runs to {:?}, but its `from` names no aura",
                        Line::SOURCE
                    ));
                }
            } else if l.to.starts_with('@') {
                self.out.push(format!(
                    "{what} runs to {:?}, which is no keyword ({:?} is the one)",
                    l.to,
                    Line::SOURCE
                ));
            } else {
                self.npc(&what, &l.to);
            }
            if !l.from.marks_a_spot() {
                self.out.push(format!(
                    "{what} starts at {:?}, which names no unit to start from",
                    l.from
                ));
            }
            // A held line lasts while an aura put on stays on, and never
            // strikes.
            if l.hold {
                if !matches!(l.from, Trigger::AuraApplied { .. }) {
                    self.out.push(format!(
                        "{what} holds, but its `from` is no `aura_applied`: only an aura going on stays on"
                    ));
                }
                if l.delay.is_some() {
                    self.out
                        .push(format!("{what} holds, and a held line has no `delay`"));
                }
                if !l.hits.is_empty() {
                    self.out.push(format!(
                        "{what} holds, and a held line never strikes, so has no `hits`"
                    ));
                }
            }
        }
    }

    fn volleys(&mut self) {
        for (k, v) in &self.e.volley {
            let what = format!("volley.{k}");
            self.trigger(&what, &v.from);
            // Round a middle of its own, its trigger need name no one.
            match &v.around {
                Some(m) => self.place_or_npc(&what, "sets out round", m),
                None if !v.from.marks_a_spot() => self.out.push(format!(
                    "{what} sets out at {:?}, which names no unit to set out from",
                    v.from
                )),
                None => {}
            }
            if v.inward && !v.start.is_some_and(|s| s.is_finite() && s > 0.0) {
                self.out.push(format!(
                    "{what} comes inward from no start: give the yards out it sets out at (`start`)"
                ));
            }
            // A spread turns each way about where it sets out: coming in,
            // the rim, so a turned one would no longer close on its middle.
            if v.inward && v.spread.is_some() {
                self.out.push(format!(
                    "{what} comes inward and spreads: a spread turns each way about its start on the rim, so it would no longer close on its middle"
                ));
            }
            // Round a middle on a trigger naming no one, there is no unit
            // to face or to keep a side of.
            if let Some(m) = &v.around
                && !v.from.marks_a_spot()
            {
                if v.aim == Aim::Facing {
                    self.out.push(format!(
                        "{what} turns with its unit's facing, but sets out round {m:?} at {:?}, which names no unit",
                        v.from
                    ));
                }
                if v.side.is_some() {
                    self.out.push(format!(
                        "{what} keeps its spots to a side of its unit, but sets out round {m:?} at {:?}, which names no unit",
                        v.from
                    ));
                }
            }
            if v.toward.is_empty() || v.toward.iter().any(|d| !d.is_finite()) {
                self.out.push(format!(
                    "{what} has no bearing, or one that is not a number"
                ));
            }
            if !(v.speed.is_finite() && v.speed > 0.0) || !(v.width.is_finite() && v.width > 0.0) {
                self.out
                    .push(format!("{what} needs a speed and a width above 0"));
            }
            if [v.start, v.length, v.delay]
                .iter()
                .flatten()
                .any(|d| !d.is_finite() || *d < 0.0)
            {
                self.out
                    .push(format!("{what} has a start, a length or a delay under 0"));
            }
            if v.every.is_some_and(|e| !(e.is_finite() && e > 0.0))
                || v.times == Some(0)
                || (v.times.is_some_and(|n| n > 1) && v.every.is_none())
            {
                self.out.push(format!(
                    "{what} is sent again with no time between, or never"
                ));
            }
            if v.spots.iter().flatten().any(|d| !d.is_finite()) {
                self.out
                    .push(format!("{what} sets out from a spot that is not a number"));
            }
            if let Some(n) = v.side.as_deref().filter(|n| *n != "raid") {
                self.npc(&what, n);
            }
            if v.faint && !v.holes {
                self.out.push(format!(
                    "{what} draws faint slots, with no holes to tell them"
                ));
            }
            if v.holes_by == HolesBy::Cluster && !v.holes {
                self.out.push(format!(
                    "{what} finds its holes by cluster, with no holes to find"
                ));
            }
            if v.alternate && v.spots.is_empty() {
                self.out
                    .push(format!("{what} alternates sides, with no spots to mirror"));
            }
            if v.holes && v.spots.len().max(1) * v.toward.len() < 2 {
                self.out.push(format!(
                    "{what} has holes, with fewer than two projectiles to leave some out"
                ));
            }
            if v.front.is_some_and(|f| !(f.is_finite() && f > 0.0)) {
                self.out.push(format!("{what} has a front, not above 0"));
            }
            if v.spread
                .is_some_and(|s| !(s.is_finite() && (0.0..180.0).contains(&s)))
            {
                self.out.push(format!(
                    "{what} spreads by {:?}°, not from 0 to under 180",
                    v.spread
                ));
            }
            for n in &v.destroys {
                self.npc(&what, n);
            }
            if v.place_kills && v.destroys.is_empty() {
                self.out.push(format!(
                    "{what} places its kills, with no NPC it destroys to place"
                ));
            }
            if v.preview && !matches!(v.from, Trigger::AuraRemoved { .. }) && !v.holds() {
                self.out.push(format!(
                    "{what} is previewed, but sets out at no aura coming off to wait on, nor after a delay to hold it still in"
                ));
            }
        }
    }

    /// A knock throws the unit its trigger is about, out from a place or
    /// an NPC, and lands it inside a place's circle where it says one.
    fn knocks(&mut self) {
        for (k, n) in &self.e.knock {
            let what = format!("knock.{k}");
            self.trigger(&what, &n.on);
            if !n.on.marks_a_spot() {
                self.out.push(format!(
                    "{what} throws on {:?}, which names no unit to throw",
                    n.on
                ));
            }
            self.place_or_npc(&what, "throws from", &n.from);
            if let Some(w) = &n.within {
                self.names(&what, "lands within", Kind::Place, w);
                if self.e.place.get(w).is_some_and(|p| p.radius.is_none()) {
                    self.out
                        .push(format!("{what}: place {w:?} has no radius to land within"));
                }
            }
        }
    }

    fn ranges(&mut self) {
        for (k, r) in &self.e.range {
            let what = format!("range.{k}");
            for n in &r.npc {
                self.npc(&what, n);
            }
            if r.radius.is_none() && r.spell.is_none() {
                self.out.push(format!(
                    "{what} has no radius and no spell to take one from"
                ));
            }
            if r.sides && r.npc.len() != 2 {
                self.out
                    .push(format!("{what} has `sides` but not two NPCs"));
            }
        }
    }

    fn aparts(&mut self) {
        for (k, a) in &self.e.apart {
            let what = format!("apart.{k}");
            for n in &a.npc {
                self.npc(&what, n);
            }
            if a.npc.len() != 2 {
                self.out
                    .push(format!("{what} names {} NPCs, not two", a.npc.len()));
            }
            for p in &a.allow {
                self.names(&what, "allows", Kind::Phase, p);
            }
        }
    }

    fn streaks(&mut self) {
        for (k, s) in &self.e.streak {
            if let Some(n) = &s.by {
                self.npc(&format!("streak.{k}"), n);
            }
        }
    }

    fn rooms(&mut self) {
        for (k, r) in &self.e.room {
            let what = format!("room.{k}");
            if let Some(a) = &r.about {
                self.names(&what, "is about", Kind::Ability, a);
            }
            if let Some(p) = &r.through {
                self.names(&what, "names", Kind::Place, p);
            } else if r.center.is_none() || r.radius.is_none() {
                self.out.push(format!(
                    "{what} is entered through no place, so it needs a center and a radius"
                ));
            }
            for n in &r.npc {
                self.npc(&what, n);
            }
            for t in r.enter.iter().chain(&r.leave) {
                self.trigger(&what, t);
            }
            if r.scale.is_some_and(|s| !s.is_finite() || s <= 0.0) {
                self.out
                    .push(format!("{what} has a scale that is not above 0"));
            }
            if !r.enter.is_empty() && r.through.is_none() {
                self.out.push(format!(
                    "{what} has an enter but no place to be entered through"
                ));
            }
        }
    }

    fn reaches(&mut self) {
        for (k, r) in &self.e.reach {
            let what = format!("reach.{k}");
            self.npc(&what, &r.npc);
            self.names(&what, "names", Kind::Place, &r.place);
            if self
                .e
                .place
                .get(&r.place)
                .is_some_and(|p| p.radius.is_none())
            {
                self.out.push(format!(
                    "{what}: place {:?} has no radius to reach",
                    r.place
                ));
            }
        }
    }

    fn events(&mut self) {
        for (k, v) in &self.e.event {
            self.trigger(&format!("event.{k}"), &v.on);
        }
    }

    fn map(&mut self) {
        let map = &self.e.map;
        for (i, s) in map.stencil.iter().enumerate() {
            self.shape(format!("map.stencil[{i}]"), s);
        }
        if map.ppy.is_some_and(|p| !(1.0..=64.0).contains(&p)) {
            self.out
                .push("map.ppy is not between 1 and 64 pixels a yard".into());
        }
        for (k, g) in &map.glow {
            let what = format!("map.glow.{k}");
            if g.area.is_empty() {
                self.out.push(format!("{what} has no area"));
            }
            for (i, s) in g.area.iter().enumerate() {
                self.shape(format!("{what}.area[{i}]"), s);
            }
            for (i, s) in g.except.iter().enumerate() {
                self.shape(format!("{what}.except[{i}]"), s);
            }
            if let Some(t) = &g.on {
                self.trigger(&what, t);
            }
            let times = [g.rise, g.hold, g.fade, g.below];
            if times.iter().flatten().any(|v| !v.is_finite() || *v < 0.0) {
                self.out.push(format!("{what} has a time or depth under 0"));
            }
        }
        for (k, l) in &map.layer {
            let what = format!("map.layer.{k}");
            match &l.breaks {
                Breaks::Phase(p) => self.names(&what, "breaks on", Kind::Phase, p),
                Breaks::Event(v) => self.names(&what, "breaks on", Kind::Event, v),
                Breaks::Occupancy | Breaks::Never => {}
            }
        }
        if let Some([x0, y0, x1, y1]) = map.crop
            && !([x0, y0, x1, y1].iter().all(|v| v.is_finite()) && x0 < x1 && y0 < y1)
        {
            self.out
                .push("map.crop is no box: [x0, y0, x1, y1] needs x0 < x1 and y0 < y1".into());
        }
        for (what, kept, left) in [
            ("group", &map.include_groups, &map.exclude_groups),
            ("model", &map.include_models, &map.exclude_models),
        ] {
            if let Some(id) = kept.iter().find(|id| left.contains(id)) {
                self.out
                    .push(format!("map: {what} {id} is both included and excluded"));
            }
        }
        if let Some(Datum::Height(h)) = map.ground
            && !h.is_finite()
        {
            self.out
                .push("map.ground is a height that is not a number".into());
        }
        if map.exposure.is_some_and(|x| !positive(x)) {
            self.out.push("map.exposure is not above 0".into());
        }
        if map.hang.is_some_and(|h| !positive(h)) {
            self.out.push("map.hang is not above 0".into());
        }
        // Which fade names each group and model, the first by key.
        let mut faded: BTreeMap<(&str, u32), &str> = BTreeMap::new();
        for (k, f) in &map.fade {
            let what = format!("map.fade.{k}");
            if f.group.is_some() == f.model.is_some() {
                self.out.push(format!(
                    "{what} names {} of `group` and `model`; it fades exactly one",
                    if f.group.is_some() { "both" } else { "neither" }
                ));
            }
            if !(f.alpha.is_finite() && f.alpha > 0.0 && f.alpha < 1.0) {
                self.out.push(format!(
                    "{what} has an alpha of {}, not above 0 and under 1",
                    f.alpha
                ));
            }
            for (word, id, left) in [
                ("group", f.group, &map.exclude_groups),
                ("model", f.model, &map.exclude_models),
            ] {
                let Some(id) = id else { continue };
                if left.contains(&id) {
                    self.out.push(format!(
                        "{what} fades {word} {id}, which the map leaves out"
                    ));
                }
                if let Some(other) = faded.insert((word, id), k) {
                    self.out.push(format!(
                        "{what} fades {word} {id}, which map.fade.{other} fades too"
                    ));
                }
            }
        }
        // Which area is each boss's court, the first by key.
        let mut courts: BTreeMap<u32, &str> = BTreeMap::new();
        for (k, a) in &map.area {
            let what = format!("map.area.{k}");
            if a.shape.is_empty() {
                self.out.push(format!("{what} has no shape"));
            }
            for (i, s) in a.shape.iter().enumerate() {
                self.shape(format!("{what}.shape[{i}]"), s);
            }
            // Every altitude, the unsaid ones as a reader fills them.
            let [far, home, near] = a.altitudes(&self.e.view);
            if [far, home, near].iter().any(|v| !positive(*v)) {
                self.out
                    .push(format!("{what} has an altitude that is not above 0"));
            } else if !(far <= home && home <= near) {
                self.out.push(format!(
                    "{what}'s altitudes are out of order: far {far} ≤ home {home} ≤ near {near} does not hold (unsaid ones are the view's)"
                ));
            }
            if a.turn.is_some_and(|t| !t.is_finite()) {
                self.out
                    .push(format!("{what} turns by an angle that is not a number"));
            }
            if a.ceiling.is_some_and(|c| !positive(c)) {
                self.out
                    .push(format!("{what} has a ceiling that is not above 0"));
            }
            if a.hang.is_some_and(|h| !positive(h)) {
                self.out
                    .push(format!("{what} has a hang that is not above 0"));
            }
            // A level's render ends at it; an arena's is checked as one,
            // though not drawn.
            for (i, s) in a.stencil.iter().enumerate() {
                self.shape(format!("{what}.stencil[{i}]"), s);
            }
            if let Some(id) = a.encounter
                && let Some(other) = courts.insert(id, k)
            {
                self.out.push(format!(
                    "{what} is encounter {id}'s court, as map.area.{other} is"
                ));
            }
        }
        for (k, p) in &map.path {
            let what = format!("map.path.{k}");
            for end in [&p.from, &p.to] {
                if !map.area.contains_key(end) {
                    self.out.push(format!(
                        "{what} names area {end:?}, which the map does not have"
                    ));
                }
            }
            if p.from == p.to {
                self.out
                    .push(format!("{what} runs from area {:?} to itself", p.from));
            }
            if [p.width, p.speed].iter().flatten().any(|v| !positive(*v)) {
                self.out
                    .push(format!("{what} has a width or a speed not above 0"));
            }
            if p.points.iter().flatten().any(|v| !v.is_finite()) {
                self.out
                    .push(format!("{what} has a point that is not a number"));
            }
            if p.spell.is_some() && p.mode != Mode::Leap {
                self.out.push(format!(
                    "{what} names a spell, but only a leap's device casts one"
                ));
            }
        }
        if let Some(w) = &map.water {
            for (key, v) in [("shallow", w.shallow), ("deep", w.deep)] {
                if !(v.is_finite() && (0.0..=1.0).contains(&v)) {
                    self.out
                        .push(format!("map.water's `{key}` is not from 0 to 1"));
                }
            }
            if !positive(w.depth) {
                self.out.push("map.water's `depth` is not above 0".into());
            }
            if w.shore.is_some_and(|y| !(y.is_finite() && y >= 0.0)) {
                self.out
                    .push("map.water's `shore` is not 0 or more yards".into());
            }
            if w.types.as_ref().is_some_and(Vec::is_empty) {
                self.out.push(
                    "map.water's `types` names none: unsaid, it draws the default light's waters"
                        .into(),
                );
            }
        }
        if let Some(t) = &map.transition {
            if t.after.is_some_and(|a| !(a.is_finite() && a >= 0.0)) {
                self.out
                    .push("map.transition's `after` is not 0 or more seconds".into());
            }
            if t.port_yd.is_some_and(|y| !positive(y)) {
                self.out
                    .push("map.transition's `port_yd` is not above 0".into());
            }
        }
    }

    /// The paths' speeds: each above 0.
    fn travel(&mut self, t: &Travel) {
        if [t.run, t.mount, t.vehicle]
            .iter()
            .flatten()
            .any(|v| !positive(*v))
        {
            self.out
                .push("travel has a speed that is not above 0".into());
        }
    }

    /// Every entry's `only`, which names difficulties: one that names none
    /// would keep the entry off every difficulty but the one it misspells.
    fn only(&mut self) {
        // Every collection of entries `only` can keep to difficulties, so a
        // new one does not build here until it is checked or said not to be.
        let Encounter {
            npc,
            ability,
            phase,
            place,
            ground,
            mark,
            line,
            volley,
            flash,
            knock,
            range,
            apart,
            streak,
            room,
            reach,
            event,
            schema: _,
            encounter: _,
            name: _,
            instance: _,
            order: _,
            map: _,
            view: _,
            spell: _,
            travel: _,
        } = self.e;
        self.only_in("npc", npc);
        self.only_in("ability", ability);
        self.only_in("phase", phase);
        self.only_in("place", place);
        self.only_in("ground", ground);
        self.only_in("mark", mark);
        self.only_in("line", line);
        self.only_in("volley", volley);
        self.only_in("flash", flash);
        self.only_in("knock", knock);
        self.only_in("range", range);
        self.only_in("apart", apart);
        self.only_in("streak", streak);
        self.only_in("room", room);
        self.only_in("reach", reach);
        self.only_in("event", event);
    }

    /// Every color an entry is drawn in: a place's, a ground's, a line's, a
    /// volley's, a flash's, a knock's and each of a range's `tint`, and a
    /// mark's `ring` and `dot`, each one the rubric names (`tint::NAMES`).
    fn tints(&mut self) {
        let e = self.e;
        let mut said: Vec<(String, &str)> = Vec::new();
        for (k, p) in &e.place {
            said.extend(p.tint.as_deref().map(|t| (format!("place.{k}.tint"), t)));
        }
        for (k, g) in &e.ground {
            said.extend(g.tint.as_deref().map(|t| (format!("ground.{k}.tint"), t)));
        }
        for (k, m) in &e.mark {
            said.extend(m.ring.as_deref().map(|t| (format!("mark.{k}.ring"), t)));
            said.extend(m.dot.as_deref().map(|t| (format!("mark.{k}.dot"), t)));
        }
        for (k, l) in &e.line {
            said.extend(l.tint.as_deref().map(|t| (format!("line.{k}.tint"), t)));
        }
        for (k, v) in &e.volley {
            said.extend(v.tint.as_deref().map(|t| (format!("volley.{k}.tint"), t)));
        }
        for (k, f) in &e.flash {
            said.extend(f.tint.as_deref().map(|t| (format!("flash.{k}.tint"), t)));
        }
        for (k, n) in &e.knock {
            said.extend(n.tint.as_deref().map(|t| (format!("knock.{k}.tint"), t)));
        }
        for (k, r) in &e.range {
            for (i, t) in r.tint.iter().enumerate() {
                said.push((format!("range.{k}.tint[{i}]"), t));
            }
        }
        for (what, t) in said {
            if !tint::known(t) {
                self.out.push(format!(
                    "{what} is {t:?}, which is not a tint ({})",
                    tint::NAMES.join(", ")
                ));
            }
        }
    }

    fn only_in<T: Gated>(&mut self, section: &str, entries: &BTreeMap<String, T>) {
        for (k, entry) in entries {
            for o in entry.only() {
                if !difficulty::NAMES.contains(&o.as_str()) {
                    self.out.push(format!(
                        "{section}.{k} is kept to {o:?}, which is not a difficulty ({})",
                        difficulty::NAMES.join(", ")
                    ));
                }
            }
        }
    }
}
