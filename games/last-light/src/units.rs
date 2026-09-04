//! Last Light's roster, production identifiers, and presentation balance.

use aurora_engine::{ArmorClass, DamageType, FactionId, ProductId, ProductionRecipe, ResourceCost};
use serde::{Deserialize, Serialize};

pub const PLAYER: FactionId = FactionId(1);
pub const CHOIR: FactionId = FactionId(2);
const WARDEN_PRODUCT: ProductId = ProductId(0);
const ENGINEER_PRODUCT: ProductId = ProductId(1);
const SURVEYOR_PRODUCT: ProductId = ProductId(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    /// Seconds between authored weapon pulses. Damage remains tuned as DPS;
    /// the simulation converts it into one pulse so animation, audio, and hit
    /// reactions receive meaningful beats instead of a 60 Hz damage stream.
    pub attack_period: f32,
    pub damage_type: DamageType,
    pub armor_class: ArmorClass,
    pub armor: f32,
    pub elevation: i8,
}

/// One-shot area-denial payload used by specialist enemy units. Keeping this
/// separate from continuous combat DPS makes the Bell Mine's detonation
/// readable and prevents callers from inferring a burst from a magic number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetonationProfile {
    pub radius: f32,
    pub damage: f32,
}

/// Campaign-facing job identity. These labels are deliberately simulation
/// data rather than UI copy so menus, tutorials, and headless tests can all
/// describe the same tactical contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum UnitRole {
    Anchor,
    Support,
    Logistics,
    Skirmisher,
    Artillery,
    Trap,
}

#[allow(dead_code)]
impl UnitRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Anchor => "FRONTLINE ANCHOR",
            Self::Support => "REPAIR SUPPORT",
            Self::Logistics => "SCOUT / HAULER",
            Self::Skirmisher => "SKIRMISHER",
            Self::Artillery => "SIEGE ARTILLERY",
            Self::Trap => "AREA DENIAL",
        }
    }
}

/// Stable production deployment stats. Mission-authored specialists may
/// override these values at spawn, but every queued unit uses this table so
/// the production card and the simulation cannot silently disagree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProductionStats {
    pub role: UnitRole,
    pub max_health: f32,
    pub speed: f32,
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

    pub const fn role(self) -> UnitRole {
        match self {
            Self::Warden => UnitRole::Anchor,
            Self::Engineer => UnitRole::Support,
            Self::Surveyor => UnitRole::Logistics,
            Self::Needle => UnitRole::Skirmisher,
            Self::Canticle => UnitRole::Artillery,
            Self::BellMine => UnitRole::Trap,
        }
    }

    pub const fn detonation(self) -> Option<DetonationProfile> {
        match self {
            Self::BellMine => Some(DetonationProfile {
                radius: 130.0,
                damage: 72.0,
            }),
            Self::Warden | Self::Engineer | Self::Surveyor | Self::Needle | Self::Canticle => None,
        }
    }

    pub const fn production_stats(self) -> Option<ProductionStats> {
        match self {
            Self::Warden => Some(ProductionStats {
                role: UnitRole::Anchor,
                max_health: 155.0,
                speed: 175.0,
            }),
            Self::Engineer => Some(ProductionStats {
                role: UnitRole::Support,
                max_health: 115.0,
                speed: 150.0,
            }),
            Self::Surveyor => Some(ProductionStats {
                role: UnitRole::Logistics,
                max_health: 90.0,
                speed: 215.0,
            }),
            Self::Needle | Self::Canticle | Self::BellMine => None,
        }
    }

    pub const fn resource_cost(self) -> ResourceCost {
        match self {
            Self::Warden => ResourceCost::new(90, 0),
            Self::Engineer => ResourceCost::new(70, 0),
            Self::Surveyor => ResourceCost::new(60, 1),
            Self::Needle | Self::Canticle | Self::BellMine => ResourceCost::new(0, 0),
        }
    }

    pub const fn supply_cost(self) -> u32 {
        match self {
            Self::Canticle => 4,
            Self::BellMine => 2,
            _ => 1,
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
                attack_period: 0.50,
                damage_type: DamageType::Normal,
                armor_class: ArmorClass::Large,
                armor: 2.0,
                elevation: 0,
            },
            Self::Engineer => CombatProfile {
                range: 90.0,
                damage_per_second: 10.0,
                attack_period: 0.70,
                damage_type: DamageType::Concussive,
                armor_class: ArmorClass::Medium,
                armor: 1.0,
                elevation: 0,
            },
            Self::Surveyor => CombatProfile {
                range: 225.0,
                damage_per_second: 15.0,
                attack_period: 0.65,
                damage_type: DamageType::Explosive,
                armor_class: ArmorClass::Small,
                armor: 0.0,
                elevation: 1,
            },
            Self::Needle => CombatProfile {
                range: 170.0,
                damage_per_second: 11.0,
                attack_period: 0.55,
                damage_type: DamageType::Concussive,
                armor_class: ArmorClass::Small,
                armor: 1.0,
                elevation: 0,
            },
            Self::Canticle => CombatProfile {
                range: 250.0,
                damage_per_second: 16.0,
                attack_period: 0.80,
                damage_type: DamageType::Explosive,
                armor_class: ArmorClass::Large,
                armor: 4.0,
                elevation: 1,
            },
            Self::BellMine => CombatProfile {
                range: 105.0,
                damage_per_second: 24.0,
                attack_period: 0.70,
                damage_type: DamageType::Explosive,
                armor_class: ArmorClass::Structure,
                armor: 3.0,
                elevation: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UnitKind, UnitRole};

    #[test]
    fn player_roles_have_distinct_jobs_and_production_profiles() {
        assert_eq!(UnitKind::Warden.role(), UnitRole::Anchor);
        assert_eq!(UnitKind::Engineer.role(), UnitRole::Support);
        assert_eq!(UnitKind::Surveyor.role(), UnitRole::Logistics);

        let warden = UnitKind::Warden
            .production_stats()
            .expect("Warden is producible");
        let engineer = UnitKind::Engineer
            .production_stats()
            .expect("Engineer is producible");
        let surveyor = UnitKind::Surveyor
            .production_stats()
            .expect("Surveyor is producible");
        assert!(warden.max_health > engineer.max_health);
        assert!(surveyor.speed > warden.speed);
        assert_eq!(UnitKind::Needle.production_stats(), None);
        let mine = UnitKind::BellMine
            .detonation()
            .expect("Bell Mine is area denial");
        assert!(mine.radius > UnitKind::BellMine.combat().range);
        assert!(mine.damage > UnitKind::BellMine.combat().damage_per_second);
        assert_eq!(UnitKind::Warden.detonation(), None);
    }

    #[test]
    fn production_recipe_and_resource_cost_stay_in_sync() {
        for kind in [UnitKind::Warden, UnitKind::Engineer, UnitKind::Surveyor] {
            let recipe = kind.recipe().expect("player roster is producible");
            assert_eq!(recipe.cost, kind.resource_cost().primary);
            assert!(recipe.build_millis > 0);
        }
    }

    #[test]
    fn combat_profiles_create_tactical_tradeoffs() {
        let warden = UnitKind::Warden.combat();
        let surveyor = UnitKind::Surveyor.combat();
        let engineer = UnitKind::Engineer.combat();
        let bell_mine = UnitKind::BellMine.combat();

        assert!(surveyor.range > warden.range);
        assert!(engineer.damage_per_second < warden.damage_per_second);
        assert!(bell_mine.damage_per_second > surveyor.damage_per_second);
        assert!(bell_mine.range < warden.range);
        assert!(warden.damage_per_second > UnitKind::Canticle.combat().damage_per_second);
        assert!(warden.attack_period > 0.0);
        assert!(UnitKind::Canticle.combat().attack_period > warden.attack_period);
    }
}
