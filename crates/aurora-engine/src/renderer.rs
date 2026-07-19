//! GPU renderer: clear + multi-texture sprite batches + optional debug triangle.

use std::sync::Arc;

use glam::Mat4;
use winit::window::Window;

use crate::camera::Camera2D;
use crate::color::Color;
use crate::post::{PostFxSettings, PostPipeline, PostUniforms};
use crate::sprite::{
    camera_uniform, CameraUniform, QueuedSprite, Sprite, SpriteBatch, SpriteVertex,
};
use crate::texture::Texture;

/// Stable handle returned when a texture is registered with the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextureHandle(pub(crate) usize);

/// Per-frame render counters for debug HUDs and performance tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub queued_sprites: usize,
    pub drawn_sprites: usize,
    pub draw_calls: usize,
}

/// Shared GPU objects games use to create textures.
pub struct GpuContext<'a> {
    /// Logical device.
    pub device: &'a wgpu::Device,
    /// Command queue.
    pub queue: &'a wgpu::Queue,
    /// Layout for sprite texture + sampler bind group.
    pub texture_bind_group_layout: &'a wgpu::BindGroupLayout,
    /// Default linear clamp sampler for sprites.
    pub sprite_sampler: &'a wgpu::Sampler,
}

/// Core GPU state and 2D draw API.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    clear_color: Color,

    // Sprite pipeline
    sprite_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    sprite_sampler: wgpu::Sampler,
    batch: SpriteBatch,
    textures: Vec<Texture>,
    draw_queue: Vec<QueuedSprite>,

    // Debug triangle (NDC)
    tri_pipeline: wgpu::RenderPipeline,
    tri_vbo: wgpu::Buffer,
    tri_uniform: wgpu::Buffer,
    tri_bind_group: wgpu::BindGroup,
    show_debug_triangle: bool,

    post: PostPipeline,
    /// Full-screen post effects (bloom, vignette, chromatic).
    pub post_fx: PostFxSettings,

    pub camera: Camera2D,
    stats: RenderStats,
    #[allow(dead_code)]
    window: Arc<Window>,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .expect("failed to create wgpu surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("failed to find a suitable GPU adapter");

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Aurora Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: if cfg!(target_arch = "wasm32") {
                        wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                    } else {
                        wgpu::Limits::default()
                    },
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .expect("failed to create GPU device");

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        // FIFO is the portable VSync mode. Selecting it deliberately keeps
        // Aurora's 60 Hz simulation from presenting with unstable pacing.
        let present_mode = surface_caps
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .unwrap_or(surface_caps.present_modes[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let sprite_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sprite Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Texture BGL"),
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
                ],
            });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera UB"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera BG"),
            layout: &camera_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let sprite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sprite.wgsl").into()),
        });

        let sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite PL"),
            bind_group_layouts: &[&camera_bgl, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Linear floating-point scene color preserves emissive lights for bloom
        // before the post pass tonemaps to the display surface.
        let scene_format = wgpu::TextureFormat::Rgba16Float;

        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Pipeline"),
            layout: Some(&sprite_layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: Some("vs_main"),
                buffers: &[SpriteVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: scene_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Debug triangle (NDC, original M0) ---
        let tri_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Triangle Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/triangle.wgsl").into()),
        });

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct TriVertex {
            position: [f32; 2],
            color: [f32; 3],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct TriUniform {
            time: f32,
            _pad: [f32; 3],
        }

        let tri_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tri UB"),
            size: std::mem::size_of::<TriUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tri_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tri BGL"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tri_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tri BG"),
            layout: &tri_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: tri_uniform.as_entire_binding(),
            }],
        });
        let tri_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tri PL"),
            bind_group_layouts: &[&tri_bgl],
            push_constant_ranges: &[],
        });
        let tri_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tri Pipeline"),
            layout: Some(&tri_pl),
            vertex: wgpu::VertexState {
                module: &tri_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<TriVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &tri_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: scene_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let tri_verts = [
            TriVertex {
                position: [0.0, 0.55],
                color: [0.15, 0.95, 0.75],
            },
            TriVertex {
                position: [-0.5, -0.45],
                color: [0.55, 0.25, 0.98],
            },
            TriVertex {
                position: [0.5, -0.45],
                color: [0.98, 0.25, 0.55],
            },
        ];
        let tri_vbo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tri VB"),
            size: std::mem::size_of_val(&tri_verts) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&tri_vbo, 0, bytemuck::cast_slice(&tri_verts));

        let batch = SpriteBatch::new(&device, SpriteBatch::DEFAULT_CAPACITY);
        let camera = Camera2D::new(width as f32, height as f32);
        let post = PostPipeline::new(&device, width, height, surface_format);

        log::info!(
            "Aurora renderer ready — adapter: {:?}, format: {:?}",
            adapter.get_info().name,
            surface_format
        );

        Self {
            surface,
            device,
            queue,
            config,
            size: winit::dpi::PhysicalSize::new(width, height),
            clear_color: Color::AURORA_NIGHT,
            sprite_pipeline,
            camera_buffer,
            camera_bind_group,
            texture_bind_group_layout,
            sprite_sampler,
            batch,
            textures: Vec::new(),
            draw_queue: Vec::with_capacity(1024),
            tri_pipeline,
            tri_vbo,
            tri_uniform,
            tri_bind_group,
            show_debug_triangle: false,
            post,
            post_fx: PostFxSettings::default(),
            camera,
            stats: RenderStats::default(),
            window,
        }
    }

    /// Borrow GPU handles for creating textures / buffers.
    pub fn gpu(&self) -> GpuContext<'_> {
        GpuContext {
            device: &self.device,
            queue: &self.queue,
            texture_bind_group_layout: &self.texture_bind_group_layout,
            sprite_sampler: &self.sprite_sampler,
        }
    }

    pub fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.size
    }

    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    pub fn set_debug_triangle(&mut self, enabled: bool) {
        self.show_debug_triangle = enabled;
    }

    pub fn debug_triangle(&self) -> bool {
        self.show_debug_triangle
    }

    /// Upload a texture and return a stable, typed handle.
    pub fn add_texture(&mut self, texture: Texture) -> TextureHandle {
        let idx = self.textures.len();
        self.textures.push(texture);
        TextureHandle(idx)
    }

    pub fn texture(&self, handle: TextureHandle) -> Option<&Texture> {
        self.textures.get(handle.0)
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Queue a sprite for the next frame (call during `on_update`).
    pub fn draw_sprite(&mut self, texture: TextureHandle, sprite: Sprite) {
        if texture.0 < self.textures.len() {
            self.draw_queue.push(QueuedSprite { texture, sprite });
        }
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.camera
                .set_viewport(new_size.width as f32, new_size.height as f32);
            self.post
                .resize(&self.device, new_size.width, new_size.height);
        }
    }

    pub fn render(&mut self, elapsed: f32) -> Result<(), wgpu::SurfaceError> {
        let vp: Mat4 = self.camera.view_projection();
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&camera_uniform(vp)),
        );

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct TriUniform {
            time: f32,
            _pad: [f32; 3],
        }
        self.queue.write_buffer(
            &self.tri_uniform,
            0,
            bytemuck::bytes_of(&TriUniform {
                time: elapsed,
                _pad: [0.0; 3],
            }),
        );

        // Sort and take ownership so we can build meshes before the pass.
        // Preserve transparent layer order; texture is only a secondary key.
        self.draw_queue.sort_by(|a, b| {
            a.sprite
                .z
                .total_cmp(&b.sprite.z)
                .then_with(|| a.texture.0.cmp(&b.texture.0))
        });
        let queue = std::mem::take(&mut self.draw_queue);
        self.stats = RenderStats {
            queued_sprites: queue.len(),
            drawn_sprites: queue.len(),
            draw_calls: 0,
        };
        self.batch.ensure_capacity(&self.device, queue.len());

        struct DrawRange {
            texture: TextureHandle,
            index_start: u32,
            index_count: u32,
        }

        let mut all_vertices: Vec<crate::sprite::SpriteVertex> = Vec::new();
        let mut all_indices: Vec<u32> = Vec::new();
        let mut ranges: Vec<DrawRange> = Vec::new();
        {
            let mut i = 0;
            while i < queue.len() {
                let tex = queue[i].texture;
                let index_start = all_indices.len() as u32;
                let vert_base_start = all_vertices.len() as u32;
                let mut local_verts = 0u32;
                while i < queue.len() && queue[i].texture == tex {
                    push_sprite_mesh(&queue[i].sprite, &mut all_vertices, &mut all_indices);
                    local_verts += 4;
                    i += 1;
                    let _ = (vert_base_start, local_verts);
                }
                let index_count = all_indices.len() as u32 - index_start;
                if index_count > 0 {
                    self.stats.draw_calls += 1;
                    ranges.push(DrawRange {
                        texture: tex,
                        index_start,
                        index_count,
                    });
                }
            }
        }

        if !all_vertices.is_empty() {
            self.queue.write_buffer(
                self.batch.vertex_buffer(),
                0,
                bytemuck::cast_slice(&all_vertices),
            );
            self.queue.write_buffer(
                self.batch.index_buffer(),
                0,
                bytemuck::cast_slice(&all_indices),
            );
        }

        let post_u = PostUniforms::from_settings(
            &self.post_fx,
            elapsed,
            self.config.width,
            self.config.height,
        );
        self.queue
            .write_buffer(&self.post.uniform_buffer, 0, bytemuck::bytes_of(&post_u));

        let output = self.surface.get_current_texture()?;
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Pass 1: scene → offscreen
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color.to_wgpu()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            if self.show_debug_triangle {
                pass.set_pipeline(&self.tri_pipeline);
                pass.set_bind_group(0, &self.tri_bind_group, &[]);
                pass.set_vertex_buffer(0, self.tri_vbo.slice(..));
                pass.draw(0..3, 0..1);
            }

            if !ranges.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.batch.vertex_buffer().slice(..));
                pass.set_index_buffer(
                    self.batch.index_buffer().slice(..),
                    wgpu::IndexFormat::Uint32,
                );

                for range in &ranges {
                    let Some(texture) = self.textures.get(range.texture.0) else {
                        continue;
                    };
                    pass.set_bind_group(1, &texture.bind_group, &[]);
                    let start = range.index_start;
                    let end = start + range.index_count;
                    pass.draw_indexed(start..end, 0, 0..1);
                }
            }
        }

        // Pass 2: post → swapchain
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Post Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.post.pipeline);
            pass.set_bind_group(0, &self.post.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}

fn push_sprite_mesh(
    sprite: &Sprite,
    vertices: &mut Vec<crate::sprite::SpriteVertex>,
    indices: &mut Vec<u32>,
) {
    use crate::sprite::SpriteVertex;
    use glam::Vec2;

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
    let base = vertices.len() as u32;
    for i in 0..4 {
        let p = corners[i];
        let rotated = Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
        let world = sprite.position + rotated;
        vertices.push(SpriteVertex {
            position: [world.x, world.y, sprite.z],
            uv: uvs[i].into(),
            color: col,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}
