//! Post-process settings and GPU resources.

use bytemuck::{Pod, Zeroable};

/// Maximum number of analytic point lights composed in the post pass.
///
/// This stays deliberately small so the same shader and uniform layout work
/// on native wgpu and the browser's conservative WebGL2 limits.
pub(crate) const MAX_POINT_LIGHTS: usize = 16;

/// A point light after world-space coordinates have been converted to screen
/// UVs by the renderer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScreenLight {
    pub position_uv: [f32; 2],
    pub radius_uv: f32,
    pub intensity: f32,
    pub color: [f32; 3],
}

/// Tunable full-screen effects applied after the scene pass.
#[derive(Debug, Clone)]
pub struct PostFxSettings {
    pub enabled: bool,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub vignette: f32,
    pub chromatic: f32,
    /// Animated film-grain strength in `0..=1`; `0` (the default) disables it.
    pub film_grain: f32,
    /// Radial dash-streak strength in `0..=1`; `0` (the default) disables it.
    pub speed_streaks: f32,
    /// Enables analytic point-light composition before bloom and tonemapping.
    pub lighting: bool,
}

impl Default for PostFxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bloom_threshold: 0.55,
            bloom_intensity: 0.85,
            vignette: 0.55,
            chromatic: 0.004,
            film_grain: 0.0,
            speed_streaks: 0.0,
            lighting: true,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct PostUniforms {
    pub time: f32,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub vignette_strength: f32,
    pub chromatic: f32,
    pub enabled: f32,
    pub texel_x: f32,
    pub texel_y: f32,
    pub light_count: f32,
    pub film_grain: f32,
    pub speed_streaks: f32,
    pub _pad: [f32; 1],
    pub lights: [[f32; 4]; MAX_POINT_LIGHTS],
    pub light_colors: [[f32; 4]; MAX_POINT_LIGHTS],
}

impl PostUniforms {
    pub fn from_settings(
        s: &PostFxSettings,
        time: f32,
        width: u32,
        height: u32,
        lights: &[ScreenLight],
    ) -> Self {
        let mut uniforms = Self {
            time,
            bloom_threshold: s.bloom_threshold,
            bloom_intensity: s.bloom_intensity,
            vignette_strength: s.vignette,
            chromatic: s.chromatic,
            enabled: if s.enabled { 1.0 } else { 0.0 },
            texel_x: 1.0 / width.max(1) as f32,
            texel_y: 1.0 / height.max(1) as f32,
            light_count: if s.lighting {
                lights.len().min(MAX_POINT_LIGHTS) as f32
            } else {
                0.0
            },
            film_grain: s.film_grain.clamp(0.0, 1.0),
            speed_streaks: s.speed_streaks.clamp(0.0, 1.0),
            _pad: [0.0; 1],
            lights: [[0.0; 4]; MAX_POINT_LIGHTS],
            light_colors: [[0.0; 4]; MAX_POINT_LIGHTS],
        };

        if s.lighting {
            for (index, light) in lights.iter().take(MAX_POINT_LIGHTS).enumerate() {
                uniforms.lights[index] = [
                    light.position_uv[0],
                    light.position_uv[1],
                    light.radius_uv.max(0.0001),
                    light.intensity.max(0.0),
                ];
                uniforms.light_colors[index] = [
                    light.color[0].max(0.0),
                    light.color[1].max(0.0),
                    light.color[2].max(0.0),
                    0.0,
                ];
            }
        }
        uniforms
    }
}

/// Offscreen scene target + post pipeline state.
pub(crate) struct PostPipeline {
    pub scene_texture: wgpu::Texture,
    pub scene_view: wgpu::TextureView,
    pub scene_sampler: wgpu::Sampler,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    pub uniform_buffer: wgpu::Buffer,
    pub pipeline: wgpu::RenderPipeline,
    pub format: wgpu::TextureFormat,
}

impl PostPipeline {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let format = wgpu::TextureFormat::Rgba16Float;
        let (scene_texture, scene_view) = create_scene_target(device, width, height, format);

        let scene_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Post Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Post BGL"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Post UB"),
            size: std::mem::size_of::<PostUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = make_bind_group(
            device,
            &bind_group_layout,
            &scene_view,
            &scene_sampler,
            &uniform_buffer,
        );

        let pipeline = build_post_pipeline(
            device,
            &bind_group_layout,
            surface_format,
            include_str!("../shaders/post.wgsl"),
        );

        Self {
            scene_texture,
            scene_view,
            scene_sampler,
            bind_group_layout,
            bind_group,
            uniform_buffer,
            pipeline,
            format,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let (tex, view) = create_scene_target(device, width, height, self.format);
        self.scene_texture = tex;
        self.scene_view = view;
        self.bind_group = make_bind_group(
            device,
            &self.bind_group_layout,
            &self.scene_view,
            &self.scene_sampler,
            &self.uniform_buffer,
        );
    }
}

fn create_scene_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Scene RT"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Builds the post pipeline from a WGSL source so shader hot reload can swap
/// it without touching the scene target or bind group.
pub(crate) fn build_post_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    surface_format: wgpu::TextureFormat,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Post Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Post PL"),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Post Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn make_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniforms: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Post BG"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniforms.as_entire_binding(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::{PostFxSettings, PostUniforms};

    #[test]
    fn speed_streaks_default_to_disabled() {
        let settings = PostFxSettings::default();
        assert_eq!(settings.speed_streaks, 0.0);
        let uniforms = PostUniforms::from_settings(&settings, 0.0, 1280, 720, &[]);
        assert_eq!(uniforms.speed_streaks, 0.0);
    }

    #[test]
    fn speed_streaks_clamp_into_the_unit_range_when_mapped_to_uniforms() {
        let settings = PostFxSettings {
            speed_streaks: 2.5,
            ..Default::default()
        };
        let uniforms = PostUniforms::from_settings(&settings, 0.0, 1280, 720, &[]);
        assert_eq!(uniforms.speed_streaks, 1.0);

        let settings = PostFxSettings {
            speed_streaks: -1.0,
            ..Default::default()
        };
        let uniforms = PostUniforms::from_settings(&settings, 0.0, 1280, 720, &[]);
        assert_eq!(uniforms.speed_streaks, 0.0);
    }
}
