//! Last Light's embedded asset catalog.
//!
//! These bytes remain embedded so native and browser builds share one loading
//! contract. The engine supplies the generic manifest/key types; this game
//! owns the names, files, and presentation meaning.

use aurora_engine::{AssetKey, AssetKind, AssetManifest, GpuContext, Texture};

#[derive(Debug, Clone, Copy)]
pub enum TextureAsset {
    ReactorSector,
    Units,
    WardenMove,
    EngineerMove,
    SurveyorScan,
    NeedleAttack,
    CanticleCommand,
    BellMineArm,
    HitReactions,
    DownReactions,
    Structures,
}

impl TextureAsset {
    pub const ALL: [Self; 11] = [
        Self::ReactorSector,
        Self::Units,
        Self::WardenMove,
        Self::EngineerMove,
        Self::SurveyorScan,
        Self::NeedleAttack,
        Self::CanticleCommand,
        Self::BellMineArm,
        Self::HitReactions,
        Self::DownReactions,
        Self::Structures,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::ReactorSector => "sector.reactor.floor",
            Self::Units => "units.idle",
            Self::WardenMove => "lantern.warden.move",
            Self::EngineerMove => "lantern.engineer.move",
            Self::SurveyorScan => "lantern.surveyor.scan",
            Self::NeedleAttack => "choir.needle.attack",
            Self::CanticleCommand => "choir.canticle.command",
            Self::BellMineArm => "choir.bell_mine.arm",
            Self::HitReactions => "units.reactions.hit",
            Self::DownReactions => "units.reactions.down",
            Self::Structures => "structures.reactor_sector",
        }
    }
    pub fn path(self) -> &'static str {
        match self {
            Self::ReactorSector => "reactor-sector-v001.png",
            Self::Units => "last-light-units-atlas-v001.png",
            Self::WardenMove => "warden-move-strip-v001.png",
            Self::EngineerMove => "engineer-move-strip-v001.png",
            Self::SurveyorScan => "surveyor-scan-strip-v001.png",
            Self::NeedleAttack => "needle-attack-strip-v001.png",
            Self::CanticleCommand => "canticle-command-strip-v001.png",
            Self::BellMineArm => "bell-mine-arm-strip-v001.png",
            Self::HitReactions => "unit-hit-reactions-atlas-v001.png",
            Self::DownReactions => "unit-down-reactions-atlas-v001.png",
            Self::Structures => "last-light-structures-atlas-v001.png",
        }
    }
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::ReactorSector => include_bytes!("../assets/reactor-sector-v001.png"),
            Self::Units => include_bytes!("../assets/last-light-units-atlas-v001.png"),
            Self::WardenMove => include_bytes!("../assets/warden-move-strip-v001.png"),
            Self::EngineerMove => include_bytes!("../assets/engineer-move-strip-v001.png"),
            Self::SurveyorScan => include_bytes!("../assets/surveyor-scan-strip-v001.png"),
            Self::NeedleAttack => include_bytes!("../assets/needle-attack-strip-v001.png"),
            Self::CanticleCommand => include_bytes!("../assets/canticle-command-strip-v001.png"),
            Self::BellMineArm => include_bytes!("../assets/bell-mine-arm-strip-v001.png"),
            Self::HitReactions => include_bytes!("../assets/unit-hit-reactions-atlas-v001.png"),
            Self::DownReactions => include_bytes!("../assets/unit-down-reactions-atlas-v001.png"),
            Self::Structures => include_bytes!("../assets/last-light-structures-atlas-v001.png"),
        }
    }
}

pub fn manifest() -> AssetManifest {
    let mut manifest = AssetManifest::new();
    for asset in TextureAsset::ALL {
        manifest
            .insert(
                AssetKey::new(asset.key()).expect("static key"),
                AssetKind::Texture,
                asset.path(),
            )
            .expect("unique static asset");
    }
    manifest
}

pub fn load_texture(gpu: &GpuContext, asset: TextureAsset) -> Texture {
    Texture::from_bytes(gpu, asset.bytes(), asset.key())
        .expect("shipped Last Light texture must decode")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_safe_asset_keys() {
        assert_eq!(manifest().len(), TextureAsset::ALL.len());
    }
}
