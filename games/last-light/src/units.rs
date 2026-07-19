//! Last Light's roster, production identifiers, and presentation balance.

use aurora_engine::{FactionId, ProductId, ProductionRecipe};

pub const PLAYER: FactionId = FactionId(1);
pub const CHOIR: FactionId = FactionId(2);
const WARDEN_PRODUCT: ProductId = ProductId(0);
const ENGINEER_PRODUCT: ProductId = ProductId(1);
const SURVEYOR_PRODUCT: ProductId = ProductId(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Warden,
    Engineer,
    Surveyor,
    Needle,
    Canticle,
    BellMine,
}

/// Combat identity used by targeting, damage, and presentation. Keeping this
/// beside the roster avoids a second, drifting table of tactical balance data.
#[derive(Debug, Clone, Copy)]
pub struct CombatProfile {
    pub range: f32,
    pub damage_per_second: f32,
}

impl UnitKind {
    pub fn from_product(product: ProductId) -> Option<Self> {
        match product {
            WARDEN_PRODUCT => Some(Self::Warden),
            ENGINEER_PRODUCT => Some(Self::Engineer),
            SURVEYOR_PRODUCT => Some(Self::Surveyor),
            _ => None,
        }
    }
    pub fn recipe(self) -> Option<ProductionRecipe> {
        match self {
            Self::Warden => Some(ProductionRecipe::new(WARDEN_PRODUCT, 90, 6_000)),
            Self::Engineer => Some(ProductionRecipe::new(ENGINEER_PRODUCT, 70, 5_000)),
            Self::Surveyor => Some(ProductionRecipe::new(SURVEYOR_PRODUCT, 60, 4_000)),
            Self::Needle | Self::Canticle | Self::BellMine => None,
        }
    }
    pub fn atlas_frame(self) -> u32 {
        match self {
            Self::Warden => 0,
            Self::Engineer => 1,
            Self::Surveyor => 2,
            Self::Needle => 3,
            Self::Canticle => 4,
            Self::BellMine => 5,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Warden => "WARDEN",
            Self::Engineer => "ENGINEER",
            Self::Surveyor => "SURVEYOR",
            Self::Needle => "CHOIR NEEDLE",
            Self::Canticle => "CHOIR CANTICLE",
            Self::BellMine => "BELL MINE",
        }
    }
    pub fn scale(self) -> f32 {
        match self {
            Self::Warden => 116.0,
            Self::Engineer => 108.0,
            Self::Surveyor => 105.0,
            Self::Needle => 104.0,
            Self::Canticle => 116.0,
            Self::BellMine => 96.0,
        }
    }

    /// Purposeful asymmetry makes formation and target selection matter:
    /// Wardens anchor, Surveyors poke safely, Engineers support, and the
    /// Choir has a fast skirmisher / artillery / close-defense mix.
    pub fn combat(self) -> CombatProfile {
        match self {
            Self::Warden => CombatProfile {
                range: 155.0,
                damage_per_second: 32.0,
            },
            Self::Engineer => CombatProfile {
                range: 90.0,
                damage_per_second: 10.0,
            },
            Self::Surveyor => CombatProfile {
                range: 225.0,
                damage_per_second: 15.0,
            },
            Self::Needle => CombatProfile {
                range: 170.0,
                damage_per_second: 18.0,
            },
            Self::Canticle => CombatProfile {
                range: 250.0,
                damage_per_second: 24.0,
            },
            Self::BellMine => CombatProfile {
                range: 105.0,
                damage_per_second: 34.0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::UnitKind;

    #[test]
    fn combat_profiles_create_tactical_tradeoffs() {
        let warden = UnitKind::Warden.combat();
        let surveyor = UnitKind::Surveyor.combat();
        let engineer = UnitKind::Engineer.combat();
        let bell_mine = UnitKind::BellMine.combat();

        assert!(surveyor.range > warden.range);
        assert!(engineer.damage_per_second < warden.damage_per_second);
        assert!(bell_mine.damage_per_second > warden.damage_per_second);
        assert!(bell_mine.range < warden.range);
    }
}
