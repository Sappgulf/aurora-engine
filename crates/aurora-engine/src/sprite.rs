//! Sprite batching for 2D textured quads.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2};

use crate::color::Color;
use crate::renderer::TextureHandle;
use crate::texture::Texture;

/// One sprite instance to draw this frame.
#[derive(Debug, Clone, Copy)]
pub struct Sprite {
    pub position: Vec2,
    pub size: Vec2,
    pub rotation: f32,
    pub color: Color,
    pub z: f32,
    /// UV rect min (0..1).
    pub uv_min: Vec2,
    /// UV rect max (0..1).
    pub uv_max: Vec2,
}

impl Default for Sprite {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            size: Vec2::splat(32.0),
            rotation: 0.0,
            color: Color::WHITE,
            z: 0.0,
            uv_min: Vec2::ZERO,
            uv_max: Vec2::ONE,
        }
    }
}

impl Sprite {
    pub fn new(position: Vec2, size: Vec2) -> Self {
        Self {
            position,
            size,
            ..Default::default()
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_rotation(mut self, rotation: f32) -> Self {
        self.rotation = rotation;
        self
    }

    pub fn with_z(mut self, z: f32) -> Self {
        self.z = z;
        self
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct SpriteVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl SpriteVertex {
    pub const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x4,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
}

/// Batches sprites sharing one texture for a single draw.
pub struct SpriteBatch {
    vertices: Vec<SpriteVertex>,
    indices: Vec<u32>,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    capacity_sprites: usize,
    pub(crate) count: u32,
}

impl SpriteBatch {
    pub const DEFAULT_CAPACITY: usize = 4096;

    pub fn new(device: &wgpu::Device, capacity_sprites: usize) -> Self {
        let capacity_sprites = capacity_sprites.max(16);
        let v_size = (capacity_sprites * 4 * std::mem::size_of::<SpriteVertex>()) as u64;
        let i_size = (capacity_sprites * 6 * std::mem::size_of::<u32>()) as u64;

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite VB"),
            size: v_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite IB"),
            size: i_size,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            vertices: Vec::with_capacity(capacity_sprites * 4),
            indices: Vec::with_capacity(capacity_sprites * 6),
            vertex_buffer,
            index_buffer,
            capacity_sprites,
            count: 0,
        }
    }

    pub fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
        self.count = 0;
    }

    /// Grow GPU buffers before upload instead of dropping sprites above a fixed cap.
    pub fn ensure_capacity(&mut self, device: &wgpu::Device, required_sprites: usize) {
        if required_sprites <= self.capacity_sprites {
            return;
        }
        let capacity_sprites = required_sprites.next_power_of_two().max(16);
        let v_size = (capacity_sprites * 4 * std::mem::size_of::<SpriteVertex>()) as u64;
        let i_size = (capacity_sprites * 6 * std::mem::size_of::<u32>()) as u64;
        self.vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite VB (grown)"),
            size: v_size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite IB (grown)"),
            size: i_size,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.capacity_sprites = capacity_sprites;
    }

    pub fn push(&mut self, sprite: &Sprite) {
        if self.count as usize >= self.capacity_sprites {
            return;
        }

        let half = sprite.size * 0.5;
        let corners = [
            Vec2::new(-half.x, -half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
            Vec2::new(-half.x, half.y),
        ];
        let (s, c) = sprite.rotation.sin_cos();
        let uvs = [
            sprite.uv_min,
            Vec2::new(sprite.uv_max.x, sprite.uv_min.y),
            sprite.uv_max,
            Vec2::new(sprite.uv_min.x, sprite.uv_max.y),
        ];
        let col = [
            sprite.color.r,
            sprite.color.g,
            sprite.color.b,
            sprite.color.a,
        ];

        let base = self.vertices.len() as u32;
        for i in 0..4 {
            let p = corners[i];
            let rotated = Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
            let world = sprite.position + rotated;
            self.vertices.push(SpriteVertex {
                position: [world.x, world.y, sprite.z],
                uv: uvs[i].into(),
                color: col,
            });
        }
        // CCW: 0-1-2, 0-2-3
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        self.count += 1;
    }

    pub fn upload(&self, queue: &wgpu::Queue) {
        if self.count == 0 {
            return;
        }
        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&self.indices));
    }

    pub fn vertex_buffer(&self) -> &wgpu::Buffer {
        &self.vertex_buffer
    }

    pub fn index_buffer(&self) -> &wgpu::Buffer {
        &self.index_buffer
    }

    pub fn index_count(&self) -> u32 {
        self.count * 6
    }
}

/// A draw command: one texture + its batch content.
pub struct SpriteLayer {
    pub texture: TextureHandle,
}

pub(crate) fn camera_uniform(view_proj: Mat4) -> CameraUniform {
    CameraUniform {
        view_proj: view_proj.to_cols_array_2d(),
    }
}

/// Draw queue entry for multi-texture frames.
pub struct QueuedSprite {
    pub texture: TextureHandle,
    pub sprite: Sprite,
}

/// Groups sprites by texture index for fewer state changes.
pub fn sort_by_texture(queue: &mut [QueuedSprite]) {
    queue.sort_by_key(|q| q.texture.0);
}

/// Placeholder to keep Texture referenced in docs.
pub type SpriteTexture = Texture;
