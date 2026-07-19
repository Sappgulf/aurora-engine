//! # Aurora Engine
//!
//! A small, fast Rust game engine powered by **wgpu**.
//! One codebase targets **desktop** (Vulkan / Metal / DX12) and the **browser** (WebGPU).
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use aurora_engine::{run, Game, Renderer, Time};
//!
//! struct MyGame;
//!
//! impl Game for MyGame {
//!     fn on_update(&mut self, time: &Time, renderer: &mut Renderer) {
//!         // update + set clear color, etc.
//!     }
//! }
//!
//! fn main() {
//!     run(MyGame);
//! }
//! ```

pub mod app;
pub mod color;
pub mod renderer;
pub mod time;

pub use app::{run, Game, TriangleDemo};
pub use color::Color;
pub use renderer::Renderer;
pub use time::Time;

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
