//! # Aurora Engine
//!
//! Fast Rust game engine powered by **wgpu** — desktop + browser (WebGPU).

pub mod app;
pub mod atlas;
pub mod audio;
pub mod camera;
pub mod collision;
pub mod color;
pub mod input;
pub mod particles;
pub mod post;
pub mod renderer;
pub mod sprite;
pub mod texture;
pub mod time;

pub use app::{run, FrameCtx, Game, TriangleDemo};
pub use atlas::{Animation, TextureAtlas};
pub use audio::Audio;
pub use camera::Camera2D;
pub use collision::Aabb;
pub use color::Color;
pub use input::Input;
pub use particles::{ParticleSystem, RngLite, XorShift32};
pub use post::PostFxSettings;
pub use renderer::{GpuContext, Renderer};
pub use sprite::{QueuedSprite, Sprite, SpriteBatch};
pub use texture::Texture;
pub use time::Time;

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
