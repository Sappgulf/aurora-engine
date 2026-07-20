//! Last Light's embedded asset catalog.
//!
//! These bytes remain embedded so native and browser builds share one loading
//! contract. The engine supplies the generic manifest/key types; this game
//! owns the names, files, and presentation meaning.

use aurora_engine::{
    atlas::AtlasRowOrigin, AssetKey, AssetKind, AssetManifest, GpuContext, Texture, TextureAtlas,
    TextureHandle,
};
use glam::Vec2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureAsset {
    ReactorSector,
    ReactorSectorReclaim,
    ReactorSectorVoice,
    ReactorSectorTerms,
    ReactorSectorGarden,
    ReactorSectorChoir,
    ReactorSectorVesper,
    ReactorSectorHollow,
    Units,
    WardenMove,
    WardenAttack,
    EngineerMove,
    EngineerRepair,
    EngineerBuild,
    SurveyorMove,
    SurveyorScan,
    SurveyorMark,
    NeedleAttack,
    CanticleCommand,
    BellMineArm,
    HitReactions,
    DownReactions,
    Structures,
    CommandPortraits,
    ResourceNodes,
    ResourceHarvestEffects,
}

/// How a player-visible state is currently rendered.
///
/// The ledger below intentionally distinguishes an authored atlas from a
/// procedural fallback and from a state that still needs dedicated art. This
/// keeps a missing clip visible to production tooling instead of letting a
/// new unit silently reuse an idle frame forever.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtStateSource {
    /// A contiguous range in one of the catalogued atlases.
    Atlas {
        asset: TextureAsset,
        first_frame: u32,
        frame_count: u32,
    },
    /// A deliberate renderer-composed state (for example a beam or boot
    /// ring) that does not require another PNG.
    ProceduralFallback,
    /// The state is named and reserved, but still needs dedicated authored
    /// art before it can be considered complete.
    PlannedAsset,
}

#[allow(dead_code)]
impl ArtStateSource {
    pub const fn atlas(asset: TextureAsset, first_frame: u32, frame_count: u32) -> Self {
        Self::Atlas {
            asset,
            first_frame,
            frame_count,
        }
    }

    #[allow(dead_code)]
    pub const fn asset(self) -> Option<TextureAsset> {
        match self {
            Self::Atlas { asset, .. } => Some(asset),
            Self::ProceduralFallback | Self::PlannedAsset => None,
        }
    }

    #[allow(dead_code)]
    pub const fn is_ready(self) -> bool {
        !matches!(self, Self::PlannedAsset)
    }
}

/// One stable, player-facing animation/presentation state.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtStateSpec {
    pub key: &'static str,
    pub source: ArtStateSource,
    pub notes: &'static str,
}

/// Production requirements for a state that is intentionally still marked as
/// [`ArtStateSource::PlannedAsset`]. This is separate from [`TextureSpec`]
/// because the output PNG does not exist yet; recording the requirements now
/// keeps an artist or asset-generation pass from guessing at frame count,
/// anchor, or image orientation.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedArtContract {
    pub key: &'static str,
    pub output_path: &'static str,
    pub source_path: &'static str,
    pub frame_count: u8,
    pub cell_size: (u16, u16),
    pub frame_origin: FrameOrigin,
    pub anchor: ArtAnchor,
    pub fps: (u8, u8),
    pub visual_intent: &'static str,
}

/// Shared anchor choices for authored unit strips. Keeping this in the
/// catalog makes the planned clips line up with the existing move/reaction
/// strips when they eventually enter the runtime atlas.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtAnchor {
    Center,
    BottomCenter,
}

/// Reserved production contracts for the next authored art pass. An empty
/// table is intentional when every player-visible state has either a shipped
/// atlas or an explicit procedural fallback.
#[allow(dead_code)]
pub const NEXT_PASS_ART_CONTRACTS: [PlannedArtContract; 0] = [];

#[allow(dead_code)]
impl ArtStateSpec {
    pub const fn atlas(
        key: &'static str,
        asset: TextureAsset,
        first_frame: u32,
        frame_count: u32,
        notes: &'static str,
    ) -> Self {
        Self {
            key,
            source: ArtStateSource::atlas(asset, first_frame, frame_count),
            notes,
        }
    }

    pub const fn procedural(key: &'static str, notes: &'static str) -> Self {
        Self {
            key,
            source: ArtStateSource::ProceduralFallback,
            notes,
        }
    }

    pub const fn planned(key: &'static str, notes: &'static str) -> Self {
        Self {
            key,
            source: ArtStateSource::PlannedAsset,
            notes,
        }
    }
}

/// Source-of-truth coverage ledger for states that can appear in the tactical
/// view. Keeping this beside the texture catalog makes the remaining art work
/// measurable: a state cannot be accidentally “implemented” by a caller that
/// forgot to register its clip or fallback.
#[allow(dead_code)]
pub const PLAYER_VISIBLE_ART_STATES: [ArtStateSpec; 27] = [
    ArtStateSpec::atlas(
        "warden.idle",
        TextureAsset::Units,
        0,
        1,
        "base roster frame",
    ),
    ArtStateSpec::atlas(
        "warden.move",
        TextureAsset::WardenMove,
        0,
        6,
        "authored locomotion strip",
    ),
    ArtStateSpec::atlas(
        "warden.attack",
        TextureAsset::WardenAttack,
        0,
        5,
        "authored shield-lance fire strip",
    ),
    ArtStateSpec::atlas(
        "warden.hit",
        TextureAsset::HitReactions,
        0,
        4,
        "roster reaction row",
    ),
    ArtStateSpec::atlas(
        "warden.down",
        TextureAsset::DownReactions,
        0,
        4,
        "persistent wreck row",
    ),
    ArtStateSpec::atlas(
        "engineer.idle",
        TextureAsset::Units,
        1,
        1,
        "base roster frame",
    ),
    ArtStateSpec::atlas(
        "engineer.move",
        TextureAsset::EngineerMove,
        0,
        6,
        "authored locomotion strip",
    ),
    ArtStateSpec::atlas(
        "engineer.repair",
        TextureAsset::EngineerRepair,
        0,
        6,
        "repair beam and tool deployment",
    ),
    ArtStateSpec::atlas(
        "engineer.build",
        TextureAsset::EngineerBuild,
        0,
        8,
        "generated and normalized construction strip; bottom-center anchor",
    ),
    ArtStateSpec::atlas(
        "engineer.hit",
        TextureAsset::HitReactions,
        4,
        4,
        "roster reaction row",
    ),
    ArtStateSpec::atlas(
        "engineer.down",
        TextureAsset::DownReactions,
        4,
        4,
        "persistent wreck row",
    ),
    ArtStateSpec::atlas(
        "surveyor.idle",
        TextureAsset::Units,
        2,
        1,
        "base roster frame",
    ),
    ArtStateSpec::atlas(
        "surveyor.move",
        TextureAsset::SurveyorMove,
        0,
        6,
        "normalized scout locomotion strip; bottom-center anchor",
    ),
    ArtStateSpec::atlas(
        "surveyor.scan",
        TextureAsset::SurveyorScan,
        0,
        6,
        "sensor mast sweep and scan fan",
    ),
    ArtStateSpec::atlas(
        "surveyor.mark",
        TextureAsset::SurveyorMark,
        0,
        4,
        "mast lock, target bracket, cyan designation pulse, and release",
    ),
    ArtStateSpec::atlas(
        "surveyor.hit",
        TextureAsset::HitReactions,
        8,
        4,
        "roster reaction row",
    ),
    ArtStateSpec::atlas(
        "surveyor.down",
        TextureAsset::DownReactions,
        8,
        4,
        "persistent wreck row",
    ),
    ArtStateSpec::atlas(
        "needle.idle",
        TextureAsset::Units,
        3,
        1,
        "hostile tactical card frame",
    ),
    ArtStateSpec::atlas(
        "needle.attack",
        TextureAsset::NeedleAttack,
        0,
        6,
        "charge, lance, and recoil",
    ),
    ArtStateSpec::atlas(
        "canticle.idle",
        TextureAsset::Units,
        4,
        1,
        "hostile tactical card frame",
    ),
    ArtStateSpec::atlas(
        "canticle.command",
        TextureAsset::CanticleCommand,
        0,
        6,
        "command ring release",
    ),
    ArtStateSpec::atlas(
        "bell_mine.idle",
        TextureAsset::Units,
        5,
        1,
        "hostile tactical card frame",
    ),
    ArtStateSpec::atlas(
        "bell_mine.arm",
        TextureAsset::BellMineArm,
        0,
        6,
        "warning arcs and armed recoil",
    ),
    ArtStateSpec::atlas(
        "structure.online",
        TextureAsset::Structures,
        0,
        4,
        "relay, fabricator, reactor, and Choir tower frames",
    ),
    ArtStateSpec::procedural(
        "structure.offline",
        "desaturated cross and low-energy glow overlay",
    ),
    ArtStateSpec::procedural(
        "structure.boot",
        "rotating cyan boot markers and pulse overlay",
    ),
    ArtStateSpec::procedural(
        "structure.damaged",
        "amber damage wash and segmented warning overlay",
    ),
];

/// Presentation role for a shipped texture. This is deliberately separate
/// from [`AssetKind`]: a structure atlas and a unit animation strip are both
/// sprite atlases at the engine boundary, but they have different frame
/// contracts in Last Light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureRole {
    EnvironmentPlate,
    UnitAtlas,
    AnimationStrip,
    ReactionAtlas,
    StructureAtlas,
    PortraitSheet,
    ResourceAtlas,
    ResourceEffectAtlas,
}

/// Pixel-space row origin used when authoring an atlas.
///
/// The renderer's UV helper converts this image-space convention into the
/// engine's Y-up world. Keeping the convention in the game catalog makes the
/// orientation fix part of the asset contract instead of tribal knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOrigin {
    /// Row zero is the top row of the source PNG.
    TopLeft,
    /// Row zero is the bottom row of the source PNG.
    ///
    /// This is retained as a supported option so source exports can retain
    /// their exported orientation when that is the fastest path to quality.
    #[allow(dead_code)]
    BottomLeft,
}

/// Authoritative metadata for one embedded Last Light texture.
///
/// Keeping the pixel size and grid beside the asset key means the renderer,
/// browser build, and asset QA all have one source of truth. `main.rs` may
/// still choose a world-space sprite size, but it should never have to guess
/// how many frames exist or what UV grid a texture uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureSpec {
    pub asset: TextureAsset,
    pub role: TextureRole,
    pub pixel_size: (u32, u32),
    pub grid: (u32, u32),
    pub frame_origin: FrameOrigin,
}

impl TextureSpec {
    #[allow(dead_code)]
    pub const fn frame_count(self) -> u32 {
        self.grid.0 * self.grid.1
    }

    #[allow(dead_code)]
    pub const fn frame_size(self) -> (u32, u32) {
        (
            self.pixel_size.0 / self.grid.0,
            self.pixel_size.1 / self.grid.1,
        )
    }

    pub const fn kind(self) -> AssetKind {
        match self.role {
            TextureRole::EnvironmentPlate => AssetKind::Texture,
            TextureRole::UnitAtlas
            | TextureRole::AnimationStrip
            | TextureRole::ReactionAtlas
            | TextureRole::StructureAtlas
            | TextureRole::PortraitSheet
            | TextureRole::ResourceAtlas
            | TextureRole::ResourceEffectAtlas => AssetKind::SpriteAtlas,
        }
    }

    /// Enforce the presentation contract for this role, not just the PNG
    /// dimensions. A correctly encoded image can still be unusable when a
    /// portrait sheet or reaction atlas is authored with the wrong grid.
    #[allow(dead_code)]
    pub const fn validate_contract(self) -> Result<(), &'static str> {
        if self.pixel_size.0 == 0 || self.pixel_size.1 == 0 {
            return Err("texture contract requires positive pixel dimensions");
        }
        if self.grid.0 == 0 || self.grid.1 == 0 {
            return Err("texture contract requires a positive atlas grid");
        }
        if !self.pixel_size.0.is_multiple_of(self.grid.0)
            || !self.pixel_size.1.is_multiple_of(self.grid.1)
        {
            return Err("texture contract requires evenly sized atlas cells");
        }
        match self.frame_origin {
            FrameOrigin::TopLeft | FrameOrigin::BottomLeft => {}
        }

        match self.role {
            TextureRole::EnvironmentPlate if self.grid.0 != 1 || self.grid.1 != 1 => {
                Err("environment plates must use a 1x1 grid")
            }
            TextureRole::UnitAtlas if self.grid.0 != 3 || self.grid.1 != 2 => {
                Err("unit idle atlas must use a 3x2 grid")
            }
            TextureRole::AnimationStrip if self.grid.1 != 1 || self.grid.0 < 4 => {
                Err("animation strips need one row and at least four frames")
            }
            TextureRole::ReactionAtlas if self.grid.0 != 4 || self.grid.1 != 6 => {
                Err("reaction atlases must use the 4x6 roster grid")
            }
            TextureRole::StructureAtlas if self.grid.0 != 2 || self.grid.1 != 2 => {
                Err("structure atlases must use the 2x2 structure grid")
            }
            TextureRole::PortraitSheet if self.grid.0 != 3 || self.grid.1 != 2 => {
                Err("portrait sheets must use the 3x2 comms grid")
            }
            TextureRole::ResourceAtlas if self.grid.0 != 2 || self.grid.1 != 2 => {
                Err("resource atlases must use the 2x2 node grid")
            }
            TextureRole::ResourceEffectAtlas if self.grid.0 != 2 || self.grid.1 != 2 => {
                Err("resource effect atlases must use the 2x2 VFX grid")
            }
            _ => Ok(()),
        }
    }
}

impl TextureAsset {
    pub const ALL: [Self; 26] = [
        Self::ReactorSector,
        Self::ReactorSectorReclaim,
        Self::ReactorSectorVoice,
        Self::ReactorSectorTerms,
        Self::ReactorSectorGarden,
        Self::ReactorSectorChoir,
        Self::ReactorSectorVesper,
        Self::ReactorSectorHollow,
        Self::Units,
        Self::WardenMove,
        Self::WardenAttack,
        Self::EngineerMove,
        Self::EngineerRepair,
        Self::EngineerBuild,
        Self::SurveyorMove,
        Self::SurveyorScan,
        Self::SurveyorMark,
        Self::NeedleAttack,
        Self::CanticleCommand,
        Self::BellMineArm,
        Self::HitReactions,
        Self::DownReactions,
        Self::Structures,
        Self::CommandPortraits,
        Self::ResourceNodes,
        Self::ResourceHarvestEffects,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::ReactorSector => "sector.reactor.floor",
            Self::ReactorSectorReclaim => "sector.reactor.reclaim",
            Self::ReactorSectorVoice => "sector.reactor.voice",
            Self::ReactorSectorTerms => "sector.reactor.terms",
            Self::ReactorSectorGarden => "sector.reactor.garden",
            Self::ReactorSectorChoir => "sector.reactor.choir",
            Self::ReactorSectorVesper => "sector.reactor.vesper",
            Self::ReactorSectorHollow => "sector.reactor.hollow",
            Self::Units => "units.idle",
            Self::WardenMove => "lantern.warden.move",
            Self::WardenAttack => "lantern.warden.attack",
            Self::EngineerMove => "lantern.engineer.move",
            Self::EngineerRepair => "lantern.engineer.repair",
            Self::EngineerBuild => "lantern.engineer.build",
            Self::SurveyorMove => "lantern.surveyor.move",
            Self::SurveyorScan => "lantern.surveyor.scan",
            Self::SurveyorMark => "lantern.surveyor.mark",
            Self::NeedleAttack => "choir.needle.attack",
            Self::CanticleCommand => "choir.canticle.command",
            Self::BellMineArm => "choir.bell_mine.arm",
            Self::HitReactions => "units.reactions.hit",
            Self::DownReactions => "units.reactions.down",
            Self::Structures => "structures.reactor_sector",
            Self::CommandPortraits => "portraits.command",
            Self::ResourceNodes => "resources.nodes",
            Self::ResourceHarvestEffects => "resources.harvest_fx",
        }
    }

    /// Return the authoritative runtime texture contract.
    ///
    /// These values intentionally describe the files in `assets/`, including
    /// the high-resolution Warden strip. The atlas UVs remain normalized, so
    /// preserving source dimensions avoids resampling already-approved art.
    pub const fn spec(self) -> TextureSpec {
        let (role, pixel_size, grid) = match self {
            Self::ReactorSector => (TextureRole::EnvironmentPlate, (1672, 941), (1, 1)),
            Self::ReactorSectorReclaim
            | Self::ReactorSectorVoice
            | Self::ReactorSectorTerms
            | Self::ReactorSectorGarden
            | Self::ReactorSectorChoir
            | Self::ReactorSectorVesper
            | Self::ReactorSectorHollow => (TextureRole::EnvironmentPlate, (1672, 941), (1, 1)),
            Self::Units => (TextureRole::UnitAtlas, (1536, 1024), (3, 2)),
            Self::WardenMove => (TextureRole::AnimationStrip, (2172, 724), (6, 1)),
            Self::WardenAttack => (TextureRole::AnimationStrip, (1280, 256), (5, 1)),
            Self::EngineerBuild => (TextureRole::AnimationStrip, (2048, 256), (8, 1)),
            Self::EngineerMove
            | Self::EngineerRepair
            | Self::SurveyorMove
            | Self::NeedleAttack
            | Self::CanticleCommand
            | Self::BellMineArm => (TextureRole::AnimationStrip, (1536, 256), (6, 1)),
            Self::SurveyorScan => (TextureRole::AnimationStrip, (1536, 256), (6, 1)),
            Self::SurveyorMark => (TextureRole::AnimationStrip, (1024, 256), (4, 1)),
            Self::HitReactions | Self::DownReactions => {
                (TextureRole::ReactionAtlas, (1024, 1536), (4, 6))
            }
            Self::Structures => (TextureRole::StructureAtlas, (1254, 1254), (2, 2)),
            Self::CommandPortraits => (TextureRole::PortraitSheet, (768, 512), (3, 2)),
            Self::ResourceNodes => (TextureRole::ResourceAtlas, (512, 512), (2, 2)),
            Self::ResourceHarvestEffects => (TextureRole::ResourceEffectAtlas, (512, 512), (2, 2)),
        };
        TextureSpec {
            asset: self,
            role,
            pixel_size,
            grid,
            frame_origin: match self {
                Self::ResourceNodes | Self::ResourceHarvestEffects => FrameOrigin::BottomLeft,
                _ => FrameOrigin::TopLeft,
            },
        }
    }

    pub const fn kind(self) -> AssetKind {
        self.spec().kind()
    }

    /// Build the runtime atlas from the same contract used by the embedded
    /// asset manifest and validation tests. Keeping this conversion here
    /// prevents a new strip from silently acquiring a different grid or
    /// pixel-size metadata in the renderer.
    pub fn runtime_atlas(self, texture: TextureHandle) -> TextureAtlas {
        let spec = self.spec();
        assert_eq!(
            spec.validate_contract(),
            Ok(()),
            "{} violates its {:?} runtime texture contract",
            self.path(),
            spec.role
        );
        TextureAtlas::new_with_row_origin(
            texture,
            spec.grid.0,
            spec.grid.1,
            Vec2::new(spec.pixel_size.0 as f32, spec.pixel_size.1 as f32),
            match spec.frame_origin {
                FrameOrigin::TopLeft => AtlasRowOrigin::TopLeft,
                FrameOrigin::BottomLeft => AtlasRowOrigin::BottomLeft,
            },
        )
    }

    /// Return the bytes used by the renderer and by asset contract tests.
    #[allow(dead_code)]
    pub fn bytes_for_validation(self) -> &'static [u8] {
        self.bytes()
    }
    pub fn path(self) -> &'static str {
        match self {
            Self::ReactorSector => "reactor-sector-v001.png",
            Self::ReactorSectorReclaim => "reactor-sector-reclaim-v001.png",
            Self::ReactorSectorVoice => "reactor-sector-voice-v001.png",
            Self::ReactorSectorTerms => "reactor-sector-terms-v001.png",
            Self::ReactorSectorGarden => "reactor-sector-garden-v001.png",
            Self::ReactorSectorChoir => "reactor-sector-choir-v001.png",
            Self::ReactorSectorVesper => "reactor-sector-vesper-v001.png",
            Self::ReactorSectorHollow => "reactor-sector-hollow-v001.png",
            Self::Units => "last-light-units-atlas-v001.png",
            Self::WardenMove => "warden-move-strip-v001.png",
            Self::WardenAttack => "warden-attack-strip-v001.png",
            Self::EngineerMove => "engineer-move-strip-v001.png",
            Self::EngineerRepair => "engineer-repair-strip-v001.png",
            Self::EngineerBuild => "engineer-build-strip-v001.png",
            Self::SurveyorMove => "surveyor-move-strip-v001.png",
            Self::SurveyorScan => "surveyor-scan-strip-v001.png",
            Self::SurveyorMark => "surveyor-mark-strip-v001.png",
            Self::NeedleAttack => "needle-attack-strip-v001.png",
            Self::CanticleCommand => "canticle-command-strip-v001.png",
            Self::BellMineArm => "bell-mine-arm-strip-v001.png",
            Self::HitReactions => "unit-hit-reactions-atlas-v001.png",
            Self::DownReactions => "unit-down-reactions-atlas-v001.png",
            Self::Structures => "last-light-structures-atlas-v001.png",
            Self::CommandPortraits => "portraits/lantern-command-portrait-sheet-v001.png",
            Self::ResourceNodes => "resource-node-atlas-v002.png",
            Self::ResourceHarvestEffects => "resource-harvest-effects-v002.png",
        }
    }
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::ReactorSector => include_bytes!("../assets/reactor-sector-v001.png"),
            Self::ReactorSectorReclaim => {
                include_bytes!("../assets/reactor-sector-reclaim-v001.png")
            }
            Self::ReactorSectorVoice => include_bytes!("../assets/reactor-sector-voice-v001.png"),
            Self::ReactorSectorTerms => include_bytes!("../assets/reactor-sector-terms-v001.png"),
            Self::ReactorSectorGarden => include_bytes!("../assets/reactor-sector-garden-v001.png"),
            Self::ReactorSectorChoir => include_bytes!("../assets/reactor-sector-choir-v001.png"),
            Self::ReactorSectorVesper => include_bytes!("../assets/reactor-sector-vesper-v001.png"),
            Self::ReactorSectorHollow => include_bytes!("../assets/reactor-sector-hollow-v001.png"),
            Self::Units => include_bytes!("../assets/last-light-units-atlas-v001.png"),
            Self::WardenMove => include_bytes!("../assets/warden-move-strip-v001.png"),
            Self::WardenAttack => include_bytes!("../assets/warden-attack-strip-v001.png"),
            Self::EngineerMove => include_bytes!("../assets/engineer-move-strip-v001.png"),
            Self::EngineerRepair => include_bytes!("../assets/engineer-repair-strip-v001.png"),
            Self::EngineerBuild => include_bytes!("../assets/engineer-build-strip-v001.png"),
            Self::SurveyorMove => include_bytes!("../assets/surveyor-move-strip-v001.png"),
            Self::SurveyorScan => include_bytes!("../assets/surveyor-scan-strip-v001.png"),
            Self::SurveyorMark => include_bytes!("../assets/surveyor-mark-strip-v001.png"),
            Self::NeedleAttack => include_bytes!("../assets/needle-attack-strip-v001.png"),
            Self::CanticleCommand => include_bytes!("../assets/canticle-command-strip-v001.png"),
            Self::BellMineArm => include_bytes!("../assets/bell-mine-arm-strip-v001.png"),
            Self::HitReactions => include_bytes!("../assets/unit-hit-reactions-atlas-v001.png"),
            Self::DownReactions => include_bytes!("../assets/unit-down-reactions-atlas-v001.png"),
            Self::Structures => include_bytes!("../assets/last-light-structures-atlas-v001.png"),
            Self::CommandPortraits => {
                include_bytes!("../assets/portraits/lantern-command-portrait-sheet-v001.png")
            }
            Self::ResourceNodes => include_bytes!("../assets/resource-node-atlas-v002.png"),
            Self::ResourceHarvestEffects => {
                include_bytes!("../assets/resource-harvest-effects-v002.png")
            }
        }
    }
}

pub fn manifest() -> AssetManifest {
    let mut manifest = AssetManifest::new();
    for asset in TextureAsset::ALL {
        manifest
            .insert(
                AssetKey::new(asset.key()).expect("static key"),
                asset.kind(),
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
    use std::collections::HashSet;

    fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
        if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
            return None;
        }
        Some((
            u32::from_be_bytes(bytes[16..20].try_into().ok()?),
            u32::from_be_bytes(bytes[20..24].try_into().ok()?),
        ))
    }

    #[test]
    fn catalog_has_unique_safe_asset_keys() {
        assert_eq!(manifest().len(), TextureAsset::ALL.len());
    }

    #[test]
    fn catalog_matches_embedded_png_dimensions_and_grids() {
        for asset in TextureAsset::ALL {
            let spec = asset.spec();
            assert_eq!(spec.asset, asset);
            assert!(spec.grid.0 > 0 && spec.grid.1 > 0);
            assert_eq!(spec.frame_count(), spec.grid.0 * spec.grid.1);
            assert_eq!(
                png_dimensions(asset.bytes_for_validation()),
                Some(spec.pixel_size),
                "{} has drifted from its catalog dimensions",
                asset.path()
            );
            assert_eq!(
                spec.pixel_size.0 % spec.grid.0,
                0,
                "{} width is not divisible by its atlas columns",
                asset.path()
            );
            assert_eq!(
                spec.pixel_size.1 % spec.grid.1,
                0,
                "{} height is not divisible by its atlas rows",
                asset.path()
            );
            assert!(spec.frame_size().0 > 0 && spec.frame_size().1 > 0);
        }
    }

    #[test]
    fn catalog_obeys_role_specific_frame_contracts() {
        for asset in TextureAsset::ALL {
            assert_eq!(
                asset.spec().validate_contract(),
                Ok(()),
                "{} violates its {:?} presentation contract",
                asset.path(),
                asset.spec().role
            );
        }
    }

    #[test]
    fn runtime_atlas_uses_the_catalog_contract() {
        for asset in TextureAsset::ALL {
            let atlas = asset.runtime_atlas(TextureHandle::default());
            let spec = asset.spec();
            assert_eq!(atlas.columns, spec.grid.0);
            assert_eq!(atlas.rows, spec.grid.1);
            assert_eq!(
                atlas.texture_size,
                Vec2::new(spec.pixel_size.0 as f32, spec.pixel_size.1 as f32)
            );
            let expected_origin = match spec.frame_origin {
                FrameOrigin::TopLeft => AtlasRowOrigin::TopLeft,
                FrameOrigin::BottomLeft => AtlasRowOrigin::BottomLeft,
            };
            assert_eq!(atlas.row_origin, expected_origin);
        }
    }

    #[test]
    fn catalog_declares_frame_row_origins() {
        for asset in TextureAsset::ALL {
            let spec = asset.spec();
            assert!(
                matches!(
                    spec.frame_origin,
                    FrameOrigin::TopLeft | FrameOrigin::BottomLeft
                ),
                "{} must use a supported frame-origin convention",
                asset.path()
            );
        }

        let mut imported = TextureAsset::Units.spec();
        imported.frame_origin = FrameOrigin::BottomLeft;
        assert_eq!(imported.validate_contract(), Ok(()));
    }

    #[test]
    fn manifest_preserves_texture_roles() {
        let manifest = manifest();
        for asset in TextureAsset::ALL {
            let key = AssetKey::new(asset.key()).expect("static key");
            assert_eq!(manifest.get(&key).unwrap().kind, asset.kind());
        }
        assert_eq!(
            manifest
                .get(&AssetKey::new(TextureAsset::ReactorSector.key()).unwrap())
                .unwrap()
                .kind,
            AssetKind::Texture
        );
        assert_eq!(
            manifest
                .get(&AssetKey::new(TextureAsset::WardenMove.key()).unwrap())
                .unwrap()
                .kind,
            AssetKind::SpriteAtlas
        );
    }

    #[test]
    fn player_visible_art_ledger_keys_and_notes_are_stable() {
        let mut keys = HashSet::new();
        for state in PLAYER_VISIBLE_ART_STATES {
            assert!(
                keys.insert(state.key),
                "duplicate player-visible art state key: {}",
                state.key
            );
            assert!(
                !state.notes.trim().is_empty(),
                "{} needs a production note",
                state.key
            );
        }
        assert_eq!(keys.len(), PLAYER_VISIBLE_ART_STATES.len());
    }

    #[test]
    fn player_visible_atlas_ranges_fit_their_catalog_contracts() {
        for state in PLAYER_VISIBLE_ART_STATES {
            let ArtStateSource::Atlas {
                asset,
                first_frame,
                frame_count,
            } = state.source
            else {
                continue;
            };
            assert!(
                frame_count > 0,
                "{} must expose at least one frame",
                state.key
            );
            assert!(
                first_frame.saturating_add(frame_count) <= asset.spec().frame_count(),
                "{} exceeds {}'s {}-frame atlas",
                state.key,
                asset.path(),
                asset.spec().frame_count()
            );
            assert!(
                matches!(
                    asset.spec().frame_origin,
                    FrameOrigin::TopLeft | FrameOrigin::BottomLeft
                ),
                "{} must use a supported frame-origin convention",
                state.key
            );
        }
    }

    #[test]
    fn next_art_pass_gaps_are_explicit_and_not_idle_aliases() {
        let planned: Vec<_> = PLAYER_VISIBLE_ART_STATES
            .iter()
            .filter(|state| matches!(state.source, ArtStateSource::PlannedAsset))
            .map(|state| state.key)
            .collect();
        let contracted: Vec<_> = NEXT_PASS_ART_CONTRACTS
            .iter()
            .map(|contract| contract.key)
            .collect();
        assert!(planned.is_empty());
        assert_eq!(contracted, planned);
        assert!(PLAYER_VISIBLE_ART_STATES.iter().any(|state| {
            state.key == "warden.attack"
                && matches!(
                    state.source,
                    ArtStateSource::Atlas {
                        asset: TextureAsset::WardenAttack,
                        first_frame: 0,
                        frame_count: 5,
                    }
                )
        }));
        assert!(PLAYER_VISIBLE_ART_STATES.iter().any(|state| {
            state.key == "structure.damaged"
                && matches!(state.source, ArtStateSource::ProceduralFallback)
        }));
    }

    #[test]
    fn planned_art_contracts_match_normalized_strip_requirements() {
        let mut keys = HashSet::new();
        for contract in NEXT_PASS_ART_CONTRACTS {
            assert!(
                keys.insert(contract.key),
                "duplicate art contract: {}",
                contract.key
            );
            assert_eq!(contract.frame_origin, FrameOrigin::TopLeft);
            assert_eq!(contract.cell_size, (256, 256));
            assert!(
                contract.frame_count >= 4,
                "{} needs a useful animation range",
                contract.key
            );
            assert!(contract.fps.0 > 0 && contract.fps.0 <= contract.fps.1);
            assert!(contract.output_path.ends_with(".png"));
            assert!(contract.source_path.ends_with("-source.png"));
            assert!(contract.output_path.contains("games/last-light/assets/"));
            assert!(contract
                .source_path
                .contains("tools/asset-sources/last-light/"));
            assert!(!contract.visual_intent.trim().is_empty());
            assert!(PLAYER_VISIBLE_ART_STATES.iter().any(|state| {
                state.key == contract.key && matches!(state.source, ArtStateSource::PlannedAsset)
            }));
        }
        assert_eq!(keys.len(), NEXT_PASS_ART_CONTRACTS.len());
    }
}
