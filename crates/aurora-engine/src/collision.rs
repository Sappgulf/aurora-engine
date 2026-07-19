//! Axis-aligned bounding boxes for 2D collision.

use glam::Vec2;

/// Axis-aligned bounding box in world space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    pub fn from_center_size(center: Vec2, size: Vec2) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn center(self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn size(self) -> Vec2 {
        self.max - self.min
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    pub fn contains_point(self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    /// Expand / shrink evenly on both axes.
    pub fn inflated(self, amount: f32) -> Self {
        Self {
            min: self.min - Vec2::splat(amount),
            max: self.max + Vec2::splat(amount),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_corners_and_detects_touching_edges() {
        let a = Aabb::new(Vec2::new(3.0, 3.0), Vec2::new(1.0, 1.0));
        let b = Aabb::new(Vec2::new(3.0, 1.5), Vec2::new(5.0, 2.5));
        assert_eq!(a.min, Vec2::ONE);
        assert!(a.intersects(b));
    }
}
