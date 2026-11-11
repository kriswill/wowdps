//! R30: the role a player played in one fight, inferred from what they did,
//! for an edition whose log carries no spec (CONTRACT.md R30). Retail's role
//! is its spec's and never comes from here: only an edition with a
//! role-signal table (`tables.rs`) infers one.

/// What a spell says about the player who casts it (or, for a form, the
/// player under it): an id in an edition's curated role-signal table
/// (`forever/role_signals.rs`, from `tools/gen-role-signals.sh`). The
/// generator proves each kind against the client's own tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleSignal {
    /// A cast that makes an enemy attack the caster (the client's
    /// ATTACK_ME effect or MOD_TAUNT aura).
    Taunt,
    /// A tank's mitigation or threat cast that needs no shield (Revenge,
    /// Last Stand, Frenzied Regeneration, a bear's Maul).
    Mitigation,
    /// A cast the client allows only with a shield equipped (Shield Block,
    /// Shield Slam). The log shows no gear without COMBATANT_INFO, so these
    /// casts are the only sight of a shield.
    Shield,
    /// A tanking form or stance: the aura on the player (MOD_SHAPESHIFT to
    /// Bear Form, Defensive Stance).
    Form,
    /// A healing cast (the client's HEAL effect or PERIODIC_HEAL aura).
    Heal,
}

impl RoleSignal {
    /// Every kind, in code order: the generated table's codes index it.
    pub const ALL: [RoleSignal; 5] = [
        RoleSignal::Taunt,
        RoleSignal::Mitigation,
        RoleSignal::Shield,
        RoleSignal::Form,
        RoleSignal::Heal,
    ];

    /// The kind's code in the generated table: its place in [`Self::ALL`].
    pub fn code(self) -> u8 {
        match self {
            RoleSignal::Taunt => 0,
            RoleSignal::Mitigation => 1,
            RoleSignal::Shield => 2,
            RoleSignal::Form => 3,
            RoleSignal::Heal => 4,
        }
    }

    /// The inverse of [`Self::code`]; any other code is `None`.
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.get(usize::from(code)).copied()
    }

    /// The kind as the review twin and the tests spell it.
    pub fn name(self) -> &'static str {
        match self {
            RoleSignal::Taunt => "taunt",
            RoleSignal::Mitigation => "mitigation",
            RoleSignal::Shield => "shield",
            RoleSignal::Form => "form",
            RoleSignal::Heal => "heal",
        }
    }
}
