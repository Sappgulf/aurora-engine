//! # Aurora Engine
//!
//! Fast Rust game engine powered by **wgpu** — desktop + browser (WebGPU).

pub mod ai;
pub mod app;
pub mod assets;
pub mod atlas;
pub mod audio;
pub mod camera;
#[cfg(feature = "3d")]
pub mod camera3d;
pub mod collision;
pub mod color;
#[cfg(not(target_arch = "wasm32"))]
pub mod devtools;
pub mod diagnostics;
pub mod input;
pub mod loader;
#[cfg(feature = "3d")]
pub mod mesh3d;
pub mod particles;
pub mod post;
pub mod renderer;
pub mod rts;
pub mod save;
pub mod scene;
pub mod sprite;
pub mod texture;
pub mod tilemap;
pub mod time;
pub mod ui;

pub use ai::{mark_obstacles, AiParams, SimpleAggroAi};
pub use app::{run, FrameCtx, Game, TriangleDemo};
pub use assets::{
    AssetEntry, AssetKey, AssetKeyError, AssetKind, AssetManifest, AssetManifestError,
};
pub use atlas::{Animation, AnimationClip, AnimationPlayer, TextureAtlas};
pub use audio::Audio;
pub use audio::{AudioChannel, AudioMixer};
pub use camera::{Camera2D, CameraRig};
#[cfg(feature = "3d")]
pub use camera3d::Camera3D;
pub use collision::Aabb;
pub use color::Color;
pub use diagnostics::{DiagnosticSnapshot, Diagnostics};
pub use input::{ActionId, Input, InputMap, KeyBinding};
pub use loader::{AssetLoadEntry, AssetLoadQueue, AssetLoadState};
#[cfg(feature = "3d")]
pub use mesh3d::{GpuMesh, Material3D, Mesh3D, MeshError, MeshVertex};
pub use particles::{ParticleSystem, RngLite, XorShift32};
pub use post::PostFxSettings;
#[cfg(feature = "3d")]
pub use renderer::Mesh3DHandle;
pub use renderer::{GpuContext, PointLight, RenderQuality, Renderer};
pub use renderer::{RenderStats, TextureHandle};
pub use rts::{
    FactionId, FogOfWar, FogState, MinimapTransform, NavGrid, PlacementError, PlacementRules,
    PowerGrid, PowerNode, PowerNodeId, ProductId, ProductionItem, ProductionQueue,
    ProductionRecipe, QueueError, ResourceBank, RtsUnit, RtsWorld, Selection, SelectionBox, UnitId,
    UnitOrder,
};
pub use save::{SaveEnvelope, SaveError, SaveStore, DEFAULT_SAVE_SLOT};
pub use scene::{EntityId, Scene};
pub use sprite::{QueuedSprite, Sprite, SpriteBatch};
pub use texture::Texture;
pub use tilemap::{TileLayer, TileMap, TileTrigger};
pub use time::Time;
pub use ui::{BitmapText, GameFlow, GlyphCell, MenuCommand, MenuInput, MenuScreen, MenuState};

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
