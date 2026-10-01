//! The theme: the names the config spells a chrome, a density and a class
//! by, and the definitions themselves — every colour and face as plain
//! numbers (`defs`), with the colour arithmetic the chrome rests on
//! (`color`). Each GUI turns them into its own types at the edge.

use wowdps_model::Class;

mod color;
mod defs;

pub use color::*;
pub use defs::*;

// ---- the chrome -----------------------------------------------------------

/// What the window's chrome — the active tab's underline, a pressed chip's
/// border — is drawn in. `Gold` needs nobody, so it is right on the first
/// frame; `Class` is the owner's class colour, remembered in the config
/// beside the locked character so it is right on the first frame too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chrome {
    #[default]
    Gold,
    Class,
}

impl Chrome {
    /// An unknown name is not an error, as with [`Density::from_name`].
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "gold" => Some(Chrome::Gold),
            "class" => Some(Chrome::Class),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Chrome::Gold => "gold",
            Chrome::Class => "class",
        }
    }
}

/// Every class, in the game's order.
pub const CLASSES: [Class; 13] = [
    Class::Warrior,
    Class::Paladin,
    Class::Hunter,
    Class::Rogue,
    Class::Priest,
    Class::DeathKnight,
    Class::Shaman,
    Class::Mage,
    Class::Warlock,
    Class::Monk,
    Class::Druid,
    Class::DemonHunter,
    Class::Evoker,
];

/// The class a config spells by its in-game name (`Class::name`), any case.
pub fn class_named(name: &str) -> Option<Class> {
    let name = name.trim();
    CLASSES
        .into_iter()
        .find(|c| c.name().eq_ignore_ascii_case(name))
}

// ---- density --------------------------------------------------------------

/// Two densities. `Comfortable` is the window default; `Compact` reproduces
/// today's tighter metrics and is what the overlay would ask for. Their
/// pitches are each GUI's (the iced GUI's `theme::DensityPitch`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

impl Density {
    /// An unknown name is not an error: a typo in a hand-edited config falls
    /// back to the default rather than failing the whole file.
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "comfortable" => Some(Density::Comfortable),
            "compact" => Some(Density::Compact),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Density::Comfortable => "comfortable",
            Density::Compact => "compact",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_are_named_the_way_the_game_names_them() {
        for class in CLASSES {
            assert_eq!(class_named(class.name()), Some(class));
            assert_eq!(
                class_named(&format!(" {} ", class.name().to_uppercase())),
                Some(class)
            );
        }
        assert_eq!(class_named("Bard"), None);
    }

    #[test]
    fn chrome_names_round_trip_and_default_to_gold() {
        assert_eq!(Chrome::default(), Chrome::Gold);
        for c in [Chrome::Gold, Chrome::Class] {
            assert_eq!(Chrome::from_name(c.name()), Some(c));
        }
        assert_eq!(Chrome::from_name("purple"), None);
    }

    #[test]
    fn density_names_round_trip_and_default_to_comfortable() {
        let (c, t) = (Density::Comfortable, Density::Compact);
        assert_eq!(Density::from_name(c.name()), Some(c));
        assert_eq!(Density::from_name(t.name()), Some(t));
        assert_eq!(Density::from_name("cozy"), None);
        assert_eq!(Density::default(), c);
    }
}
