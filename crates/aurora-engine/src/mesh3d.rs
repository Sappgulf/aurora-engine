//! Mesh and material data for Aurora's 3D renderer, plus GPU upload helpers.

use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use crate::renderer::GpuContext;
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
        if !indices.len().is_multiple_of(3) {
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

    /// A unit cube (half-extent 0.5) with per-face normals and UVs.
    pub fn cube() -> Self {
        let h = 0.5;
        // Each face lists 4 corners in CCW order as seen from outside along
        // its normal, so the standard (0,1,2, 0,2,3) fan triangulates correctly.
        let faces: [(Vec3, [Vec3; 4]); 6] = [
            (
                Vec3::X,
                [
                    Vec3::new(h, -h, -h),
                    Vec3::new(h, h, -h),
                    Vec3::new(h, h, h),
                    Vec3::new(h, -h, h),
                ],
            ),
            (
                Vec3::NEG_X,
                [
                    Vec3::new(-h, -h, -h),
                    Vec3::new(-h, -h, h),
                    Vec3::new(-h, h, h),
                    Vec3::new(-h, h, -h),
                ],
            ),
            (
                Vec3::Y,
                [
                    Vec3::new(-h, h, -h),
                    Vec3::new(-h, h, h),
                    Vec3::new(h, h, h),
                    Vec3::new(h, h, -h),
                ],
            ),
            (
                Vec3::NEG_Y,
                [
                    Vec3::new(-h, -h, -h),
                    Vec3::new(h, -h, -h),
                    Vec3::new(h, -h, h),
                    Vec3::new(-h, -h, h),
                ],
            ),
            (
                Vec3::Z,
                [
                    Vec3::new(-h, -h, h),
                    Vec3::new(h, -h, h),
                    Vec3::new(h, h, h),
                    Vec3::new(-h, h, h),
                ],
            ),
            (
                Vec3::NEG_Z,
                [
                    Vec3::new(-h, -h, -h),
                    Vec3::new(-h, h, -h),
                    Vec3::new(h, h, -h),
                    Vec3::new(h, -h, -h),
                ],
            ),
        ];
        let uvs = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];

        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for (normal, corners) in faces {
            let base = vertices.len() as u32;
            for (corner, uv) in corners.into_iter().zip(uvs) {
                vertices.push(MeshVertex {
                    position: corner,
                    normal,
                    uv,
                });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Self::new(vertices, indices).expect("cube geometry is valid")
    }

    /// A UV sphere (radius 0.5) built from `segments` longitude slices and
    /// `rings` latitude bands.
    pub fn uv_sphere(segments: u32, rings: u32) -> Self {
        let segments = segments.max(3);
        let rings = rings.max(2);
        let radius = 0.5;

        let mut vertices = Vec::with_capacity(((segments + 1) * (rings + 1)) as usize);
        for ring in 0..=rings {
            let v = ring as f32 / rings as f32;
            let theta = v * std::f32::consts::PI;
            let (sin_theta, cos_theta) = theta.sin_cos();
            for seg in 0..=segments {
                let u = seg as f32 / segments as f32;
                let phi = u * std::f32::consts::TAU;
                let (sin_phi, cos_phi) = phi.sin_cos();
                let normal = Vec3::new(sin_theta * cos_phi, cos_theta, sin_theta * sin_phi);
                vertices.push(MeshVertex {
                    position: normal * radius,
                    normal,
                    uv: Vec2::new(u, v),
                });
            }
        }

        let stride = segments + 1;
        let mut indices = Vec::with_capacity((segments * rings * 6) as usize);
        for ring in 0..rings {
            for seg in 0..segments {
                let a = ring * stride + seg;
                let b = a + stride;
                let c = a + 1;
                let d = b + 1;
                indices.extend_from_slice(&[a, b, c, c, b, d]);
            }
        }
        Self::new(vertices, indices).expect("uv sphere geometry is valid")
    }
}

/// GPU-safe (plain, `repr(C)`) mirror of [`MeshVertex`] used for buffer uploads.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuMeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

impl From<MeshVertex> for GpuMeshVertex {
    fn from(vertex: MeshVertex) -> Self {
        Self {
            position: vertex.position.into(),
            normal: vertex.normal.into(),
            uv: vertex.uv.into(),
        }
    }
}

impl GpuMeshVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
    ];

    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// A [`Mesh3D`] uploaded to GPU vertex/index buffers, ready to draw.
pub struct GpuMesh {
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) index_count: u32,
}

impl GpuMesh {
    /// Uploads validated CPU mesh data to the GPU. Call once per unique mesh
    /// (e.g. from `Game::on_start`), then reuse the returned handle to draw
    /// many transformed instances per frame.
    pub fn upload(gpu: &GpuContext<'_>, mesh: &Mesh3D) -> Self {
        let gpu_vertices: Vec<GpuMeshVertex> = mesh
            .vertices
            .iter()
            .copied()
            .map(GpuMeshVertex::from)
            .collect();
        let vertex_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Mesh3D VB"),
                contents: bytemuck::cast_slice(&gpu_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Mesh3D IB"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        Self {
            vertex_buffer,
            index_buffer,
            index_count: mesh.indices.len() as u32,
        }
    }

    pub(crate) fn layout() -> wgpu::VertexBufferLayout<'static> {
        GpuMeshVertex::layout()
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
    fn cube_has_valid_closed_geometry() {
        let cube = Mesh3D::cube();
        assert_eq!(cube.vertices.len(), 24);
        assert_eq!(cube.indices.len(), 36);
    }

    #[test]
    fn uv_sphere_has_valid_geometry() {
        let sphere = Mesh3D::uv_sphere(8, 4);
        assert_eq!(sphere.vertices.len(), 9 * 5);
        assert_eq!(sphere.indices.len(), 8 * 4 * 6);
        // Every triangle stays within the generated vertex range (Mesh3D::new
        // already asserts this, but pin the doubled-vertex-per-seam count too).
        assert!(sphere
            .indices
            .iter()
            .all(|i| (*i as usize) < sphere.vertices.len()));
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
