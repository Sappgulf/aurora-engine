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
}
