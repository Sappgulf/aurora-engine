//! GPU textures and procedural generators.

use crate::color::Color;
use crate::renderer::GpuContext;

const MAX_TEXTURE_DIMENSION: u32 = 8_192;
const MAX_TEXTURE_BYTES: usize = 256 * 1024 * 1024;

/// Validates a decoded RGBA payload before it reaches the GPU allocator.
pub fn validate_rgba_payload(width: u32, height: u32, rgba_len: usize) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err(format!(
            "texture dimensions must be nonzero, got {width}x{height}"
        ));
    }
    if width > MAX_TEXTURE_DIMENSION || height > MAX_TEXTURE_DIMENSION {
        return Err(format!(
            "texture dimensions exceed maximum of {MAX_TEXTURE_DIMENSION}x{MAX_TEXTURE_DIMENSION}, got {width}x{height}"
        ));
    }
    let expected = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| format!("texture dimensions overflow for {width}x{height}"))?;
    let expected = usize::try_from(expected)
        .map_err(|_| "texture payload exceeds the host address space".to_owned())?;
    if expected > MAX_TEXTURE_BYTES {
        return Err(format!(
            "decoded texture payload exceeds maximum of {MAX_TEXTURE_BYTES} bytes, got {expected}"
        ));
    }
    if rgba_len != expected {
        return Err(format!(
            "texture pixel payload has wrong size: expected {expected}, got {rgba_len}"
        ));
    }
    Ok(())
}

fn pixel_len(width: u32, height: u32, channels: u32, what: &str) -> usize {
    let Some(bytes) = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|px| px.checked_mul(u64::from(channels)))
    else {
        panic!("texture dimensions too large for {what}: {width}x{height}x{channels}");
    };
    usize::try_from(bytes)
        .unwrap_or_else(|_| panic!("texture byte size exceeds address space for {what}"))
}

fn expect_pixel_len(width: u32, height: u32, rgba: &[u8], what: &str) {
    validate_rgba_payload(width, height, rgba.len())
        .unwrap_or_else(|error| panic!("invalid texture payload for {what}: {error}"));
}

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
        expect_pixel_len(width, height, rgba, "from_rgba");

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
        let (w, h) = (img.width(), img.height());
        let expected = u64::from(w)
            .checked_mul(u64::from(h))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .unwrap_or(usize::MAX);
        validate_rgba_payload(w, h, expected)?;
        let rgba = img.to_rgba8();
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
        let mut data = vec![0u8; pixel_len(size, size, 4, "soft_circle")];
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

    /// Faceted diamond pickup with a transparent background.
    pub fn crystal(gpu: &GpuContext<'_>, size: u32, color: Color) -> Self {
        let mut data = vec![0u8; pixel_len(size, size, 4, "crystal")];
        let half = (size as f32 - 1.0) * 0.5;
        for y in 0..size {
            for x in 0..size {
                let dx = (x as f32 - half) / half;
                let dy = (y as f32 - half) / half;
                let edge = dx.abs() + dy.abs();
                if edge > 0.94 {
                    continue;
                }

                let facet = if dy < -dx.abs() * 0.35 {
                    1.35
                } else if dx < 0.0 {
                    0.78
                } else {
                    1.05
                };
                let highlight = (1.0 - (dx * 0.7 + dy * 0.9).abs()).powf(2.0) * 0.28;
                let i = ((y * size + x) * 4) as usize;
                data[i] = (color.r * (facet + highlight) * 255.0).min(255.0) as u8;
                data[i + 1] = (color.g * (facet + highlight) * 255.0).min(255.0) as u8;
                data[i + 2] = (color.b * (facet + highlight) * 255.0).min(255.0) as u8;
                data[i + 3] = ((0.94 - edge) / 0.08)
                    .clamp(0.0, 1.0)
                    .sqrt()
                    .mul_add(255.0, 0.0) as u8;
            }
        }
        Self::from_rgba(gpu, size, size, &data, "crystal")
    }

    /// Checkerboard for debugging UV / camera.
    pub fn checker(gpu: &GpuContext<'_>, size: u32, cell: u32, a: Color, b: Color) -> Self {
        let mut data = vec![0u8; pixel_len(size, size, 4, "checker")];
        for y in 0..size {
            for x in 0..size {
                let on = ((x / cell) + (y / cell)) & 1 == 0;
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
        let mut data = vec![0u8; pixel_len(size, size, 4, "gradient_h")];
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

    /// A low-contrast arena material: indigo depth, cyan lane lines, and a central energy bloom.
    pub fn arena_floor(gpu: &GpuContext<'_>, size: u32) -> Self {
        let mut data = vec![0u8; pixel_len(size, size, 4, "arena_floor")];
        for y in 0..size {
            for x in 0..size {
                let u = x as f32 / (size - 1).max(1) as f32;
                let v = y as f32 / (size - 1).max(1) as f32;
                let dx = u - 0.5;
                let dy = v - 0.5;
                let distance = (dx * dx + dy * dy).sqrt();
                let bloom = (1.0 - distance * 1.8).clamp(0.0, 1.0).powf(2.5);
                let grid_x = ((u * 12.0).fract() - 0.5).abs();
                let grid_y = ((v * 7.0).fract() - 0.5).abs();
                let grid = if grid_x < 0.008 || grid_y < 0.012 {
                    1.0
                } else {
                    0.0
                };
                let lane = if (v - 0.5).abs() < 0.004 || (u - 0.5).abs() < 0.003 {
                    1.0
                } else {
                    0.0
                };
                let glow = bloom * 0.14 + grid * 0.035 + lane * 0.10;
                let i = ((y * size + x) * 4) as usize;
                data[i] = ((0.012 + glow * 0.15) * 255.0) as u8;
                data[i + 1] = ((0.025 + glow * 0.52) * 255.0) as u8;
                data[i + 2] = ((0.075 + glow * 0.82) * 255.0) as u8;
                data[i + 3] = 255;
            }
        }
        Self::from_rgba(gpu, size, size, &data, "arena_floor")
    }

    /// Horizontal strip of soft orbs with slight size variation (4 frames) for animation.
    pub fn orb_atlas_strip(
        gpu: &GpuContext<'_>,
        frame_size: u32,
        frames: u32,
        color: Color,
    ) -> Self {
        let frames = frames.max(1);
        let w = frame_size.checked_mul(frames).unwrap_or_else(|| {
            panic!("orb atlas dimensions overflow: frame_size={frame_size}, frames={frames}")
        });
        let h = frame_size;
        let mut data = vec![0u8; pixel_len(w, h, 4, "orb_atlas_strip")];
        for f in 0..frames {
            let scale = 0.75
                + 0.25
                    * ((f as f32 / frames as f32) * std::f32::consts::TAU)
                        .sin()
                        .abs();
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

#[cfg(test)]
mod tests {
    use super::validate_rgba_payload;

    #[test]
    fn rgba_payload_preflight_accepts_valid_data_and_rejects_limits() {
        assert!(validate_rgba_payload(2, 2, 16).is_ok());
        assert!(validate_rgba_payload(8_193, 1, 8_193 * 4).is_err());
        assert!(validate_rgba_payload(1, 1, 256 * 1024 * 1024 + 1).is_err());
    }
}
