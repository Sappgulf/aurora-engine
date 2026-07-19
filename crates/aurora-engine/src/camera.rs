//! 2D orthographic camera.

use glam::{Mat4, Vec2};

/// Orthographic camera in world space (Y-up, units ≈ pixels at zoom 1).
#[derive(Debug, Clone)]
pub struct Camera2D {
    /// World-space center the camera looks at.
    pub position: Vec2,
    /// Zoom factor (1.0 = 1 world unit ≈ 1 pixel).
    pub zoom: f32,
    /// Minimum / maximum zoom clamps.
    pub zoom_min: f32,
    pub zoom_max: f32,
    viewport: Vec2,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self::new(1280.0, 720.0)
    }
}

impl Camera2D {
    pub fn new(viewport_width: f32, viewport_height: f32) -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            zoom_min: 0.15,
            zoom_max: 8.0,
            viewport: Vec2::new(viewport_width.max(1.0), viewport_height.max(1.0)),
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = Vec2::new(width.max(1.0), height.max(1.0));
    }

    pub fn viewport(&self) -> Vec2 {
        self.viewport
    }

    pub fn pan(&mut self, delta_world: Vec2) {
        self.position += delta_world;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(self.zoom_min, self.zoom_max);
    }

    pub fn zoom_at(&mut self, factor: f32, screen_point: Vec2) {
        let before = self.screen_to_world(screen_point);
        self.zoom_by(factor);
        let after = self.screen_to_world(screen_point);
        self.position += before - after;
    }

    /// View-projection matrix for shaders (clip space).
    pub fn view_projection(&self) -> Mat4 {
        let half = self.viewport / (2.0 * self.zoom);
        let left = self.position.x - half.x;
        let right = self.position.x + half.x;
        let bottom = self.position.y - half.y;
        let top = self.position.y + half.y;
        Mat4::orthographic_rh(left, right, bottom, top, -1000.0, 1000.0)
    }

    /// Convert screen pixels (origin top-left) to world.
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        let ndc = Vec2::new(
            (screen.x / self.viewport.x) * 2.0 - 1.0,
            1.0 - (screen.y / self.viewport.y) * 2.0,
        );
        let half = self.viewport / (2.0 * self.zoom);
        Vec2::new(
            self.position.x + ndc.x * half.x,
            self.position.y + ndc.y * half.y,
        )
    }

    /// Convert world to screen pixels (origin top-left).
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        let half = self.viewport / (2.0 * self.zoom);
        let ndc = Vec2::new(
            (world.x - self.position.x) / half.x,
            (world.y - self.position.y) / half.y,
        );
        Vec2::new(
            (ndc.x + 1.0) * 0.5 * self.viewport.x,
            (1.0 - ndc.y) * 0.5 * self.viewport.y,
        )
    }
}
