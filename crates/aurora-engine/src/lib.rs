//! # Aurora Engine
//!
//! Fast Rust game engine powered by **wgpu** — desktop + browser (WebGPU).

pub mod app;
pub mod assets;
pub mod atlas;
pub mod audio;
pub mod camera;
#[cfg(feature = "3d")]
pub mod camera3d;
pub mod collision;
pub mod color;
pub mod input;
pub mod particles;
pub mod post;
pub mod renderer;
pub mod scene;
pub mod sprite;
pub mod texture;
pub mod tilemap;
pub mod time;

pub use app::{run, FrameCtx, Game, TriangleDemo};
pub use assets::{
    AssetEntry, AssetKey, AssetKeyError, AssetKind, AssetManifest, AssetManifestError,
};
pub use atlas::{Animation, TextureAtlas};
pub use audio::Audio;
pub use camera::Camera2D;
#[cfg(feature = "3d")]
pub use camera3d::Camera3D;
pub use collision::Aabb;
pub use color::Color;
pub use input::{Action, Input};
pub use particles::{ParticleSystem, RngLite, XorShift32};
pub use post::PostFxSettings;
pub use renderer::{GpuContext, PointLight, RenderQuality, Renderer};
pub use renderer::{RenderStats, TextureHandle};
pub use scene::{EntityId, Scene};
pub use sprite::{QueuedSprite, Sprite, SpriteBatch};
pub use texture::Texture;
pub use tilemap::{TileLayer, TileMap};
pub use time::Time;

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
