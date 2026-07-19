//! # Aurora Engine
//!
//! Fast Rust game engine powered by **wgpu** — desktop + browser (WebGPU).
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use aurora_engine::{run, FrameCtx, Game, Renderer};
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn on_update(&mut self, ctx: &mut FrameCtx<'_>) {
//!         // draw sprites, move camera, read input…
//!     }
//! }
//!
//! fn main() {
//!     run(MyGame);
//! }
//! ```

pub mod app;
pub mod camera;
pub mod color;
pub mod input;
pub mod particles;
pub mod renderer;
pub mod sprite;
pub mod texture;
pub mod time;

pub use app::{run, FrameCtx, Game, TriangleDemo};
pub use camera::Camera2D;
pub use color::Color;
pub use input::Input;
pub use particles::{ParticleSystem, RngLite, XorShift32};
pub use renderer::{GpuContext, Renderer};
pub use sprite::{QueuedSprite, Sprite, SpriteBatch};
pub use texture::Texture;
pub use time::Time;

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
