//! 2D orthographic camera.

use glam::{Mat4, Vec2};

use crate::Aabb;

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

    /// World-space size currently visible through this orthographic camera.
    ///
    /// UI and arena backgrounds should use this instead of fixed pixel-sized
    /// rectangles so they remain correct after a resize or DPI-scale change.
    pub fn visible_world_size(&self) -> Vec2 {
        self.viewport / self.zoom.max(f32::EPSILON)
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
        let half = self.visible_world_size() * 0.5;
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
        let half = self.visible_world_size() * 0.5;
        Vec2::new(
            self.position.x + ndc.x * half.x,
            self.position.y + ndc.y * half.y,
        )
    }

    /// Convert world to screen pixels (origin top-left).
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        let half = self.visible_world_size() * 0.5;
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

/// A reusable camera controller with smooth follow, world bounds, and transient shake.
#[derive(Debug, Clone)]
pub struct CameraRig {
    /// World-space point to keep near the camera center.
    pub target: Vec2,
    /// Follow responsiveness in 1/seconds. Higher values catch up faster.
    pub follow_speed: f32,
    /// Half-size of the world-space window the target may move within without panning.
    pub dead_zone: Vec2,
    /// Optional limits for the unshaken camera view.
    pub bounds: Option<Aabb>,
    anchor: Option<Vec2>,
    shake: CameraShake,
}

#[derive(Debug, Clone, Copy, Default)]
struct CameraShake {
    magnitude: f32,
    remaining: f32,
    duration: f32,
    frequency: f32,
    elapsed: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self::new(Vec2::ZERO)
    }
}

impl CameraRig {
    pub fn new(target: Vec2) -> Self {
        Self {
            target,
            follow_speed: 12.0,
            dead_zone: Vec2::splat(32.0),
            bounds: None,
            anchor: None,
            shake: CameraShake::default(),
        }
    }

    /// Force the rig and camera to its current target, useful at scene entry.
    pub fn snap_to_target(&mut self, camera: &mut Camera2D) {
        let anchor = self.clamp_to_bounds(camera, self.target);
        self.anchor = Some(anchor);
        camera.position = anchor;
    }

    /// Layer a short camera impulse on top of normal follow motion.
    pub fn shake(&mut self, magnitude: f32, duration: f32) {
        self.shake = CameraShake {
            magnitude: magnitude.max(0.0),
            remaining: duration.max(0.0),
            duration: duration.max(f32::EPSILON),
            frequency: 28.0,
            elapsed: 0.0,
        };
    }

    pub fn is_shaking(&self) -> bool {
        self.shake.remaining > 0.0
    }

    /// Advance the rig and apply its result to the supplied camera.
    pub fn update(&mut self, camera: &mut Camera2D, delta_seconds: f32) {
        let dt = delta_seconds.max(0.0);
        let current = self.anchor.unwrap_or(camera.position);
        let offset = self.target - current;
        let desired = current
            + Vec2::new(
                Self::outside_dead_zone(offset.x, self.dead_zone.x.max(0.0)),
                Self::outside_dead_zone(offset.y, self.dead_zone.y.max(0.0)),
            );
        let blend = 1.0 - (-self.follow_speed.max(0.0) * dt).exp();
        let anchor = self.clamp_to_bounds(camera, current.lerp(desired, blend));
        self.anchor = Some(anchor);

        let shake = self.update_shake(dt);
        camera.position = self.clamp_to_bounds(camera, anchor + shake);
    }

    fn outside_dead_zone(distance: f32, radius: f32) -> f32 {
        if distance.abs() <= radius {
            0.0
        } else {
            distance - distance.signum() * radius
        }
    }

    fn clamp_to_bounds(&self, camera: &Camera2D, position: Vec2) -> Vec2 {
        let Some(bounds) = self.bounds else {
            return position;
        };
        let half_view = camera.viewport() / (2.0 * camera.zoom.max(f32::EPSILON));
        let center = bounds.center();
        let min = bounds.min + half_view;
        let max = bounds.max - half_view;
        Vec2::new(
            if min.x > max.x {
                center.x
            } else {
                position.x.clamp(min.x, max.x)
            },
            if min.y > max.y {
                center.y
            } else {
                position.y.clamp(min.y, max.y)
            },
        )
    }

    fn update_shake(&mut self, dt: f32) -> Vec2 {
        if self.shake.remaining <= 0.0 {
            return Vec2::ZERO;
        }
        self.shake.remaining = (self.shake.remaining - dt).max(0.0);
        self.shake.elapsed += dt;
        let fade = (self.shake.remaining / self.shake.duration).clamp(0.0, 1.0);
        let phase = self.shake.elapsed * self.shake.frequency;
        Vec2::new((phase * 1.91).sin(), (phase * 2.53 + 0.7).sin()) * self.shake.magnitude * fade
    }
}

#[cfg(test)]
mod rig_tests {
    use super::*;

    #[test]
    fn visible_world_size_tracks_zoom_without_dpi_assumptions() {
        let mut camera = Camera2D::new(1920.0, 1080.0);
        assert_eq!(camera.visible_world_size(), Vec2::new(1920.0, 1080.0));
        camera.zoom = 1.5;
        assert_eq!(camera.visible_world_size(), Vec2::new(1280.0, 720.0));
    }

    #[test]
    fn rig_follows_target_outside_dead_zone_and_respects_bounds() {
        let mut camera = Camera2D::new(100.0, 100.0);
        let mut rig = CameraRig::new(Vec2::new(180.0, 50.0));
        rig.follow_speed = 1000.0;
        rig.dead_zone = Vec2::splat(10.0);
        rig.bounds = Some(Aabb::new(Vec2::ZERO, Vec2::new(200.0, 200.0)));
        rig.update(&mut camera, 1.0);
        assert_eq!(camera.position, Vec2::new(150.0, 50.0));

        rig.target = Vec2::new(400.0, 50.0);
        rig.update(&mut camera, 1.0);
        assert_eq!(camera.position, Vec2::new(150.0, 50.0));
    }

    #[test]
    fn shake_is_transient_and_does_not_move_the_anchor() {
        let mut camera = Camera2D::new(100.0, 100.0);
        let mut rig = CameraRig::new(Vec2::new(50.0, 50.0));
        rig.snap_to_target(&mut camera);
        rig.shake(8.0, 0.1);
        rig.update(&mut camera, 0.02);
        assert_ne!(camera.position, Vec2::new(50.0, 50.0));
        rig.update(&mut camera, 1.0);
        assert_eq!(camera.position, Vec2::new(50.0, 50.0));
        assert!(!rig.is_shaking());
    }
}
