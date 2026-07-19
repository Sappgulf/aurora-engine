//! Post-process settings and GPU resources.

use bytemuck::{Pod, Zeroable};

/// Tunable full-screen effects applied after the scene pass.
#[derive(Debug, Clone)]
pub struct PostFxSettings {
    pub enabled: bool,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    pub vignette: f32,
    pub chromatic: f32,
}

impl Default for PostFxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bloom_threshold: 0.55,
            bloom_intensity: 0.85,
            vignette: 0.55,
            chromatic: 0.004,
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
}

impl PostUniforms {
    pub fn from_settings(s: &PostFxSettings, time: f32, width: u32, height: u32) -> Self {
        Self {
            time,
            bloom_threshold: s.bloom_threshold,
            bloom_intensity: s.bloom_intensity,
            vignette_strength: s.vignette,
            chromatic: s.chromatic,
            enabled: if s.enabled { 1.0 } else { 0.0 },
            texel_x: 1.0 / width.max(1) as f32,
            texel_y: 1.0 / height.max(1) as f32,
        }
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
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Post Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/post.wgsl").into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Post PL"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
        });

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
