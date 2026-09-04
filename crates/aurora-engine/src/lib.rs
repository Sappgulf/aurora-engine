//! # Aurora Engine
//!
//! Fast Rust game engine powered by **wgpu** — desktop + browser (WebGPU).

pub mod abilities;
pub mod agent;
pub mod ai;
pub mod app;
pub mod assets;
pub mod atlas;
pub mod atlas_pack;
pub mod audio;
pub mod camera;
#[cfg(feature = "3d")]
pub mod camera3d;
pub mod collision;
pub mod color;
#[cfg(not(target_arch = "wasm32"))]
pub mod devtools;
#[cfg(not(target_arch = "wasm32"))]
pub use devtools::{DebugHarness, FileWatcher};
pub mod diagnostics;
pub mod font;
pub mod fsm;
#[cfg(feature = "3d")]
pub mod gltf;
pub mod input;
pub mod juice;
pub mod level;
pub mod level_editor;
pub mod loader;
#[cfg(feature = "3d")]
pub mod mesh3d;
pub mod music;
pub mod particles;
pub mod performance;
pub mod physics2d;
pub mod platform;
pub mod post;
pub mod profile;
pub mod renderer;
pub mod rts;
pub mod save;
pub mod scene;
pub mod sprite;
pub mod texture;
pub mod tilemap;
pub mod time;
pub mod trace;
pub mod ui;
#[cfg(target_arch = "wasm32")]
pub mod web_agent;

pub use abilities::CooldownBook;
pub use ai::{mark_obstacles, AiParams, SimpleAggroAi};
pub use app::{run, FrameCtx, Game, TriangleDemo};
pub use assets::{
    AssetEntry, AssetKey, AssetKeyError, AssetKind, AssetManifest, AssetManifestError,
};
pub use atlas::{Animation, AnimationClip, AnimationPlayer, TextureAtlas};
pub use atlas_pack::{AtlasPackError, PackedAtlas, PackedEntry};
pub use audio::{Audio, AudioChannel, AudioError, AudioMixer};
pub use camera::{Camera2D, CameraRig};
#[cfg(feature = "3d")]
pub use camera3d::Camera3D;
pub use collision::Aabb;
pub use color::Color;
pub use diagnostics::{DiagnosticSnapshot, Diagnostics, RuntimeStatus};
pub use font::{
    default_charset, glyphs_to_sprites, layout_with_source, line_width_with, measure_line_with,
    wrap_lines_with, Align, Font, FontError, GlyphAtlas, GlyphEntry, GlyphMetrics, GlyphSource,
    PositionedGlyph, ShelfPacker, TextLayout,
};
pub use fsm::StateMachine;
#[cfg(feature = "3d")]
pub use gltf::{GltfError, GltfMeshPart, GltfScene};
pub use input::{
    ActionBinding, ActionId, GamepadFrame, Input, InputMap, KeyBinding, PadButton, RumbleRequest,
    MAX_GAMEPADS,
};
pub use juice::{
    motion_intensity, parallax_offset, Easing, HitStop, LoopMode, ScheduledFire, Scheduler, Tween,
    TweenRunner, TweenValue,
};
pub use level::{
    AmbienceDef, BossDef, Level, LevelDef, LevelError, LevelLoadError, MoverDef, PickupDef,
    PowerKind, PowerUpDef, RectDef, SlopeDef, ThemeDef,
};
pub use level_editor::{
    EditResult, EditorCommand, EditorSelection, LevelEditor, LevelEditorError, LevelElement,
    MAX_UNDO_STEPS,
};
pub use loader::{AssetLoadEntry, AssetLoadQueue, AssetLoadState, AssetPriority};
#[cfg(feature = "3d")]
pub use mesh3d::{GpuMesh, Material3D, Mesh3D, MeshError, MeshVertex};
pub use music::{Melody, Note, Sequencer};
pub use particles::{
    EmitterConfig, ParticleSystem, RateEmitter, RngLite, SpawnedParticle, XorShift32,
};
pub use performance::{QualityController, QualityControllerConfig};
pub use physics2d::{
    raycast_any, step_character, CharacterParams, CollisionContext, CollisionEvent, ContactSide,
    ContactSurface, Intent, KinematicBody, Platform, RayHit, Slope, SLOPE_SNAP,
    SLOPE_WALL_THRESHOLD, WATER_DRAG, WATER_GRAVITY_SCALE, WATER_TERMINAL_FALL,
};
pub use platform::{LifecycleEvent, LifecycleState, SurfaceStatus};
pub use post::PostFxSettings;
pub use profile::{
    AccessibilityProfile, AudioProfile, ControllerProfile, DisplayProfile, EngineProfile,
};
#[cfg(feature = "3d")]
pub use renderer::Mesh3DHandle;
pub use renderer::{GpuContext, PointLight, RenderBudget, RenderQuality, Renderer};
pub use renderer::{RenderStats, TextureHandle};
#[cfg(feature = "3d")]
pub use renderer::{ShadowSettings, SkySettings};
pub use rts::{
    ArmorClass, BlobId, BlockId, BuildId, BuildItem, BuildQueue, BuildQueueError, BuildRecipe,
    CombatEvent, CombatProfile, DamageType, FactionId, FlubberBlob, FlubberId, FogOfWar, FogState,
    MinimapTransform, MotionBlob, MotionBlock, NavGrid, PlacementError, PlacementRules, PowerGrid,
    PowerNode, PowerNodeId, ProductId, ProductionCancelError, ProductionCancelReceipt,
    ProductionItem, ProductionQueue, ProductionRecipe, QueueError, ResourceBank, ResourceCost,
    ResourceSet, RtsCombatResolver, RtsUnit, RtsWorld, Selection, SelectionBox, SupplyLedger,
    SupplyQueueError, TechGraph, TechId, TerrainClass, TerrainReadout, TerrainZone, UnitId,
    UnitOrder,
};
pub use save::{LoadedSave, SaveEnvelope, SaveError, SaveSource, SaveStore, DEFAULT_SAVE_SLOT};
pub use scene::{EntityId, Scene};
pub use sprite::{QueuedSprite, Sprite, SpriteBatch};
pub use texture::Texture;
pub use tilemap::{TileLayer, TileMap, TileTrigger};
pub use time::Time;
pub use trace::{
    hash_serializable, run_trace, run_trace_with_checkpoints, AuroraTrace, DeterministicSimulation,
    SemanticCommand, StableStateHasher, StateHash, TraceCheckpoint, TraceError, TraceRunReport,
    TRACE_FORMAT_VERSION,
};
pub use ui::{
    BitmapText, GameFlow, GlyphCell, MenuCommand, MenuInput, MenuNavigator, MenuScreen, MenuState,
    SettingsTransaction,
};

/// Optional re-export of the [`hecs`] ECS, enabled with the `ecs` feature.
#[cfg(feature = "ecs")]
pub use hecs;

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
