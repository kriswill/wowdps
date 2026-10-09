//! What a resolved encounter says that cannot hold: a name it uses but
//! does not have, a size that is no size, a difficulty that is none.

use std::collections::BTreeMap;

use crate::schema::Gated;
use crate::{Breaks, Encounter, StencilShape, Trigger, difficulty};

/// What an encounter names that it does not have (an ability's NPC, a
/// pet's owner, a trigger's NPC, phase or event), and what it says that
/// cannot be (a shape that is none, a time under 0, an `only` naming no
/// difficulty). Empty when it is whole.
pub fn check(e: &Encounter) -> Vec<String> {
    let mut c = Check { e, out: Vec::new() };
    c.npcs();
    c.abilities();
    c.phases();
    c.places();
    c.grounds();
    c.lines();
    c.volleys();
    c.ranges();
    c.aparts();
    c.streaks();
    c.rooms();
    c.reaches();
    c.events();
    c.map();
    c.only();
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
}

impl Kind {
    fn word(self) -> &'static str {
        match self {
            Kind::Npc => "npc",
            Kind::Ability => "ability",
            Kind::Phase => "phase",
            Kind::Place => "place",
            Kind::Event => "event",
        }
    }
}

struct Check<'a> {
    e: &'a Encounter,
    out: Vec<String>,
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
        }
    }

    /// `what` names a `kind` of entry, saying `how` ("names", "is under",
    /// "breaks on" …): a problem when the encounter has none by that name.
    fn names(&mut self, what: &str, how: &str, kind: Kind, name: &str) {
        if !self.has(kind, name) {
            self.out.push(format!(
                "{what} {how} {} {name:?}, which the encounter does not have",
                kind.word()
            ));
        }
    }

    fn npc(&mut self, what: &str, name: &str) {
        self.names(what, "names", Kind::Npc, name);
    }

    /// A trigger's NPC, phase or event.
    fn trigger(&mut self, what: &str, t: &Trigger) {
        match t {
            Trigger::Cast { by: Some(n), .. }
            | Trigger::AuraApplied { on: Some(n), .. }
            | Trigger::AuraRemoved { on: Some(n), .. }
            | Trigger::HealthBelow { npc: n, .. }
            | Trigger::Plateau { npc: n, .. }
            | Trigger::Appears { npc: n }
            | Trigger::Death { npc: n } => self.npc(what, n),
            Trigger::Event(ev) => self.names(what, "names", Kind::Event, ev),
            Trigger::Phase(p) => self.names(what, "names", Kind::Phase, p),
            Trigger::After(c) => {
                if !c.secs.is_finite() || c.secs < 0.0 {
                    self.out.push(format!(
                        "{what} strikes {} s after it starts counting: a clock is seconds, 0 or more",
                        c.secs
                    ));
                }
                if let Some(since) = &c.since {
                    if c.nth == 0 {
                        self.out.push(format!(
                            "{what} counts from the 0th time its `since` fired; `nth` counts from 1"
                        ));
                    }
                    self.trigger(what, since);
                }
            }
            Trigger::Start
            | Trigger::Cast { by: None, .. }
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
        for (k, n) in &self.e.npc {
            for o in n.owner.iter().chain(&n.shares) {
                self.npc(&format!("npc.{k}"), o);
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
            for t in p.from.iter().chain(&p.until) {
                self.trigger(&format!("place.{k}"), t);
            }
        }
    }

    fn grounds(&mut self) {
        for (k, g) in &self.e.ground {
            let what = format!("ground.{k}");
            if let Some(o) = &g.orbit
                && !self.has(Kind::Place, &o.around)
                && !self.has(Kind::Npc, &o.around)
            {
                self.out.push(format!(
                    "{what} circles {:?}, which is neither a place nor an NPC",
                    o.around
                ));
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
        }
    }

    fn lines(&mut self) {
        for (k, l) in &self.e.line {
            let what = format!("line.{k}");
            self.trigger(&what, &l.from);
            self.npc(&what, &l.to);
            if !l.from.marks_a_spot() {
                self.out.push(format!(
                    "{what} starts at {:?}, which names no unit to start from",
                    l.from
                ));
            }
        }
    }

    fn volleys(&mut self) {
        for (k, v) in &self.e.volley {
            let what = format!("volley.{k}");
            self.trigger(&what, &v.from);
            if !v.from.marks_a_spot() {
                self.out.push(format!(
                    "{what} sets out at {:?}, which names no unit to set out from",
                    v.from
                ));
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
            if v.preview && !matches!(v.from, Trigger::AuraRemoved { .. }) {
                self.out.push(format!(
                    "{what} is previewed, but sets out at no aura coming off to wait on"
                ));
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
        self.only_in("range", range);
        self.only_in("apart", apart);
        self.only_in("streak", streak);
        self.only_in("room", room);
        self.only_in("reach", reach);
        self.only_in("event", event);
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
