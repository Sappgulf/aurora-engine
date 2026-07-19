//! Feature-gated mesh and material data for Aurora's future 3D renderer.
//!
//! This module intentionally contains only CPU-side, validated asset data.
//! Uploading buffers and drawing PBR materials belong to a later renderer pass.

use glam::{Vec2, Vec3};

use crate::Color;

/// One interleaved vertex for a right-handed 3D mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
}

/// Validated indexed triangle geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh3D {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    EmptyVertices,
    NonTriangleIndices,
    IndexOutOfBounds,
}

impl Mesh3D {
    pub fn new(vertices: Vec<MeshVertex>, indices: Vec<u32>) -> Result<Self, MeshError> {
        if vertices.is_empty() {
            return Err(MeshError::EmptyVertices);
        }
        if indices.len() % 3 != 0 {
            return Err(MeshError::NonTriangleIndices);
        }
        if indices
            .iter()
            .any(|index| *index as usize >= vertices.len())
        {
            return Err(MeshError::IndexOutOfBounds);
        }
        Ok(Self { vertices, indices })
    }

    /// A unit plane useful as a mesh upload and material smoke fixture.
    pub fn unit_plane() -> Self {
        let normal = Vec3::Y;
        let vertices = vec![
            MeshVertex {
                position: Vec3::new(-0.5, 0.0, -0.5),
                normal,
                uv: Vec2::new(0.0, 0.0),
            },
            MeshVertex {
                position: Vec3::new(0.5, 0.0, -0.5),
                normal,
                uv: Vec2::new(1.0, 0.0),
            },
            MeshVertex {
                position: Vec3::new(0.5, 0.0, 0.5),
                normal,
                uv: Vec2::new(1.0, 1.0),
            },
            MeshVertex {
                position: Vec3::new(-0.5, 0.0, 0.5),
                normal,
                uv: Vec2::new(0.0, 1.0),
            },
        ];
        Self::new(vertices, vec![0, 1, 2, 0, 2, 3]).expect("unit plane indices are valid")
    }
}

/// PBR-ready material values independent of any texture or GPU backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material3D {
    pub base_color: Color,
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: Color,
}

impl Default for Material3D {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            metallic: 0.0,
            roughness: 0.7,
            emissive: Color::BLACK,
        }
    }
}

impl Material3D {
    pub fn sanitized(mut self) -> Self {
        self.metallic = self.metallic.clamp(0.0, 1.0);
        self.roughness = self.roughness.clamp(0.04, 1.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_rejects_invalid_triangle_indices() {
        let vertex = MeshVertex {
            position: Vec3::ZERO,
            normal: Vec3::Y,
            uv: Vec2::ZERO,
        };
        assert_eq!(
            Mesh3D::new(vec![vertex], vec![0, 1, 0]),
            Err(MeshError::IndexOutOfBounds)
        );
        assert_eq!(
            Mesh3D::new(vec![vertex], vec![0, 0]),
            Err(MeshError::NonTriangleIndices)
        );
    }

    #[test]
    fn material_clamps_pbr_ranges() {
        let material = Material3D {
            metallic: 2.0,
            roughness: 0.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(material.metallic, 1.0);
        assert_eq!(material.roughness, 0.04);
    }
}
