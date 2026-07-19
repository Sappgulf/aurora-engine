//! Feature-gated 3D camera conventions for Aurora's future mesh renderer.
//!
//! Aurora uses a right-handed coordinate system: +Y is up and the camera
//! looks down its local -Z axis, matching wgpu's standard projection path.

use glam::{Mat4, Vec3};

/// Perspective camera data kept independent of any mesh or renderer backend.
#[derive(Debug, Clone)]
pub struct Camera3D {
    pub position: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub vertical_fov_radians: f32,
    pub near: f32,
    pub far: f32,
    aspect: f32,
}

impl Camera3D {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            position: Vec3::new(0.0, 1.5, 4.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            vertical_fov_radians: 60.0_f32.to_radians(),
            near: 0.05,
            far: 1_000.0,
            aspect: (width / height).max(0.001),
        }
    }

    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.aspect = (width / height).max(0.001);
    }

    pub fn view_projection(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.position, self.target, self.up);
        let projection =
            Mat4::perspective_rh(self.vertical_fov_radians, self.aspect, self.near, self.far);
        projection * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_changes_projection_aspect() {
        let mut camera = Camera3D::new(1600.0, 900.0);
        let wide = camera.view_projection();
        camera.set_viewport(900.0, 1600.0);
        assert_ne!(wide, camera.view_projection());
    }
}
