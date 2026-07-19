//! GPU textures and procedural generators.

use crate::color::Color;
use crate::renderer::GpuContext;

/// Handle to a GPU texture + bind group for the sprite sampler layout.
#[derive(Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub bind_group: wgpu::BindGroup,
}

impl Texture {
    pub fn from_rgba(
        gpu: &GpuContext<'_>,
        width: u32,
        height: u32,
        rgba: &[u8],
        label: &str,
    ) -> Self {
        assert_eq!(rgba.len(), (width * height * 4) as usize);

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{label} bind group")),
            layout: gpu.texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(gpu.sprite_sampler),
                },
            ],
        });

        Self {
            width,
            height,
            texture,
            view,
            bind_group,
        }
    }

    /// Load from PNG (or other formats enabled on the `image` crate).
    pub fn from_bytes(gpu: &GpuContext<'_>, bytes: &[u8], label: &str) -> Result<Self, String> {
        let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        Ok(Self::from_rgba(gpu, w, h, &rgba, label))
    }

    /// Solid color 1×1 (tinted via sprite color).
    pub fn solid(gpu: &GpuContext<'_>, color: Color) -> Self {
        let px = [
            (color.r * 255.0) as u8,
            (color.g * 255.0) as u8,
            (color.b * 255.0) as u8,
            (color.a * 255.0) as u8,
        ];
        Self::from_rgba(gpu, 1, 1, &px, "solid")
    }

    /// Soft circular glow (good for particles / orbs).
    pub fn soft_circle(gpu: &GpuContext<'_>, size: u32, color: Color) -> Self {
        let mut data = vec![0u8; (size * size * 4) as usize];
        let cxy = (size as f32 - 1.0) * 0.5;
        let r = size as f32 * 0.5;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - cxy;
                let dy = y as f32 - cxy;
                let d = (dx * dx + dy * dy).sqrt() / r;
                let a = (1.0 - d).clamp(0.0, 1.0).powf(1.6);
                let i = ((y * size + x) * 4) as usize;
                data[i] = (color.r * 255.0) as u8;
                data[i + 1] = (color.g * 255.0) as u8;
                data[i + 2] = (color.b * 255.0) as u8;
                data[i + 3] = (a * color.a * 255.0) as u8;
            }
        }
        Self::from_rgba(gpu, size, size, &data, "soft_circle")
    }

    /// Checkerboard for debugging UV / camera.
    pub fn checker(gpu: &GpuContext<'_>, size: u32, cell: u32, a: Color, b: Color) -> Self {
        let mut data = vec![0u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let on = ((x / cell) + (y / cell)) % 2 == 0;
                let c = if on { a } else { b };
                let i = ((y * size + x) * 4) as usize;
                data[i] = (c.r * 255.0) as u8;
                data[i + 1] = (c.g * 255.0) as u8;
                data[i + 2] = (c.b * 255.0) as u8;
                data[i + 3] = (c.a * 255.0) as u8;
            }
        }
        Self::from_rgba(gpu, size, size, &data, "checker")
    }

    /// Horizontal gradient.
    pub fn gradient_h(gpu: &GpuContext<'_>, size: u32, left: Color, right: Color) -> Self {
        let mut data = vec![0u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let t = x as f32 / (size - 1).max(1) as f32;
                let i = ((y * size + x) * 4) as usize;
                data[i] = ((left.r + (right.r - left.r) * t) * 255.0) as u8;
                data[i + 1] = ((left.g + (right.g - left.g) * t) * 255.0) as u8;
                data[i + 2] = ((left.b + (right.b - left.b) * t) * 255.0) as u8;
                data[i + 3] = ((left.a + (right.a - left.a) * t) * 255.0) as u8;
            }
        }
        Self::from_rgba(gpu, size, size, &data, "gradient_h")
    }

    /// Horizontal strip of soft orbs with slight size variation (4 frames) for animation.
    pub fn orb_atlas_strip(gpu: &GpuContext<'_>, frame_size: u32, frames: u32, color: Color) -> Self {
        let frames = frames.max(1);
        let w = frame_size * frames;
        let h = frame_size;
        let mut data = vec![0u8; (w * h * 4) as usize];
        for f in 0..frames {
            let scale = 0.75 + 0.25 * ((f as f32 / frames as f32) * std::f32::consts::TAU).sin().abs();
            let cxy = (frame_size as f32 - 1.0) * 0.5;
            let r = frame_size as f32 * 0.5 * scale;
            for y in 0..frame_size {
                for x in 0..frame_size {
                    let dx = x as f32 - cxy;
                    let dy = y as f32 - cxy;
                    let d = (dx * dx + dy * dy).sqrt() / r.max(1.0);
                    let a = (1.0 - d).clamp(0.0, 1.0).powf(1.4);
                    let px = f * frame_size + x;
                    let i = ((y * w + px) * 4) as usize;
                    data[i] = (color.r * 255.0) as u8;
                    data[i + 1] = (color.g * 255.0) as u8;
                    data[i + 2] = (color.b * 255.0) as u8;
                    data[i + 3] = (a * color.a * 255.0) as u8;
                }
            }
        }
        Self::from_rgba(gpu, w, h, &data, "orb_atlas")
    }
}
