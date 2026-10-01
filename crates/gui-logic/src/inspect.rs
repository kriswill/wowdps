//! The window inspector's model, free of any GUI (plan step 3.3, wave B):
//! who is who ([`Roster`]), the graph's curves and dead spans ([`plot`]),
//! its lanes ([`lanes`]) and its stacked bands' seating ([`stack`]) —
//! moved from the iced window's inspector, so both window GUIs build the
//! same graph from the same snapshot.

use std::collections::HashMap;

use wowdps_model::{Class, Row, Spec};

pub mod lanes;
pub mod plot;
pub mod stack;

/// Everyone the window has seen on a meter, by guid: their name and class
/// — what a lane's span is coloured by when its caster is not the player
/// (the Heroism's Shaman is on the Damage rows, and perhaps not on the
/// Taken ones the inspector shows). A guid's class never changes, so the
/// roster only grows, and a class learned is never forgotten.
///
/// It answers by NAME too: a drill's target rows are keyed by name and
/// wear the DRILLED player's class (the model's "drilldown rows alike"),
/// so who a target is — a player in their colour, or a creature — is the
/// roster's to say.
#[derive(Debug, Clone, Default)]
pub struct Roster {
    people: HashMap<String, (String, Option<Class>)>,
    names: HashMap<String, (Option<Class>, Option<Spec>)>,
}

impl Roster {
    /// Take in a meter's rows (players: never the enemies' view).
    pub fn observe(&mut self, rows: &[Row]) {
        for r in rows {
            match self.people.get_mut(&r.key) {
                Some(have) if have.1.is_none() && r.class.is_some() => have.1 = r.class,
                Some(_) => {}
                None => {
                    self.people
                        .insert(r.key.clone(), (r.label.clone(), r.class));
                }
            }
            // Every tick walks every row: a name already known is looked up,
            // and only a new one is allocated for.
            if !self.names.contains_key(&r.label) {
                self.names.insert(r.label.clone(), (None, None));
            }
            if let Some(named) = self.names.get_mut(&r.label) {
                if r.class.is_some() {
                    named.0 = r.class;
                }
                if r.spec.is_some() {
                    named.1 = r.spec;
                }
            }
        }
    }

    /// Their name ("Name-Realm") and class, when seen.
    pub fn get(&self, guid: &str) -> Option<(&str, Option<Class>)> {
        self.people.get(guid).map(|(n, c)| (n.as_str(), *c))
    }

    /// `rows` — a drill's targets, keyed by name — each wearing its OWN
    /// class and spec: a player the roster has seen in theirs, anything
    /// else none (a creature).
    pub fn as_themselves(&self, rows: &[Row]) -> Vec<Row> {
        rows.iter()
            .map(|r| {
                let (class, spec) = self.names.get(&r.label).copied().unwrap_or_default();
                Row {
                    class,
                    spec,
                    ..r.clone()
                }
            })
            .collect()
    }
}
