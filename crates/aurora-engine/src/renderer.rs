//! GPU renderer: clear + multi-texture sprite batches + optional debug triangle.

use std::sync::Arc;

#[cfg(feature = "3d")]
use glam::Vec3;
use glam::{Mat4, Vec2};
use winit::window::Window;

use crate::camera::Camera2D;
#[cfg(feature = "3d")]
use crate::camera3d::Camera3D;
use crate::color::Color;
#[cfg(feature = "3d")]
use crate::mesh3d::{GpuMesh, Material3D, Mesh3D};
use crate::post::{PostFxSettings, PostPipeline, PostUniforms, ScreenLight, MAX_POINT_LIGHTS};
use crate::sprite::{
    camera_uniform, sprite_corner_uvs, CameraUniform, QueuedSprite, Sprite, SpriteBatch,
    SpriteVertex,
};
use crate::texture::Texture;
use crate::time::InstantCompat;

/// Stable handle returned when a texture is registered with the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextureHandle(pub(crate) usize);

/// Per-frame render counters for debug HUDs and performance tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub queued_sprites: usize,
    pub drawn_sprites: usize,
    pub draw_calls: usize,
    pub queued_lights: usize,
    pub composed_lights: usize,
    /// CPU time spent encoding and presenting the most recent frame.
    pub cpu_frame_ms: f32,
}

/// A colored radial light composed over the HDR scene before bloom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// Center in world coordinates.
    pub position: Vec2,
    /// Linear HDR color. Values above 1.0 are valid for intense neon light.
    pub color: Color,
    /// Radius in world units.
    pub radius: f32,
    /// Brightness multiplier applied after distance falloff.
    pub intensity: f32,
}

impl PointLight {
    pub const fn new(position: Vec2, color: Color, radius: f32, intensity: f32) -> Self {
        Self {
            position,
            color,
            radius,
            intensity,
        }
    }
}

/// Portable lighting budget presets. The simulation is unaffected; this only
/// limits the number of lights composed by the renderer each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderQuality {
    Performance,
    #[default]
    Balanced,
    Cinematic,
}

impl RenderQuality {
    const fn light_budget(self) -> usize {
        match self {
            Self::Performance => 4,
            Self::Balanced => 8,
            Self::Cinematic => MAX_POINT_LIGHTS,
        }
    }
}

/// Stable handle to a mesh uploaded with [`Renderer::upload_mesh3d`].
#[cfg(feature = "3d")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mesh3DHandle(pub(crate) usize);

#[cfg(feature = "3d")]
struct QueuedMesh3D {
    mesh: Mesh3DHandle,
    transform: Mat4,
    material: Material3D,
}

/// Camera + single directional light state written to the scene uniform
/// each frame. Kept CPU-side so games can call the setters in any order.
#[cfg(feature = "3d")]
#[derive(Debug, Clone, Copy)]
struct Scene3DState {
    view_proj: Mat4,
    camera_pos: Vec3,
    light_dir: Vec3,
    light_color: Color,
    light_intensity: f32,
}

#[cfg(feature = "3d")]
impl Default for Scene3DState {
    fn default() -> Self {
        Self {
            view_proj: Mat4::IDENTITY,
            camera_pos: Vec3::ZERO,
            light_dir: Vec3::new(-0.4, -1.0, -0.3).normalize(),
            light_color: Color::WHITE,
            light_intensity: 1.0,
        }
    }
}

#[cfg(feature = "3d")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Scene3DUniform {
    view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
}

#[cfg(feature = "3d")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Object3DUniform {
    model: [[f32; 4]; 4],
    base_color: [f32; 4],
    emissive: [f32; 4],
    metallic_roughness: [f32; 4],
}

#[cfg(feature = "3d")]
fn create_depth_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Mesh3D Depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
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
    /// Queued (not just latest-wins) so a screenshot requested during a
    /// transient render hiccup isn't silently dropped by a later request.
    #[cfg(not(target_arch = "wasm32"))]
    pending_captures: Vec<std::path::PathBuf>,

    // Sprite pipeline
    sprite_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    sprite_sampler: wgpu::Sampler,
    batch: SpriteBatch,
    textures: Vec<Texture>,
    draw_queue: Vec<QueuedSprite>,
    light_queue: Vec<PointLight>,

    // Debug triangle (NDC)
    tri_pipeline: wgpu::RenderPipeline,
    tri_vbo: wgpu::Buffer,
    tri_uniform: wgpu::Buffer,
    tri_bind_group: wgpu::BindGroup,
    show_debug_triangle: bool,

    post: PostPipeline,
    /// Full-screen post effects (bloom, vignette, chromatic).
    pub post_fx: PostFxSettings,
    quality: RenderQuality,

    pub camera: Camera2D,
    /// Current window scale used to convert physical surface pixels into the
    /// camera/HUD's logical viewport. Kept explicitly because winit can emit a
    /// scale-factor event before the next resize event.
    scale_factor: f64,
    stats: RenderStats,
    #[allow(dead_code)]
    window: Arc<Window>,

    // Mesh3D pipeline (depth-tested, single directional light PBR)
    #[cfg(feature = "3d")]
    #[allow(dead_code)]
    depth_texture: wgpu::Texture,
    #[cfg(feature = "3d")]
    depth_view: wgpu::TextureView,
    #[cfg(feature = "3d")]
    mesh3d_pipeline: wgpu::RenderPipeline,
    #[cfg(feature = "3d")]
    mesh3d_scene_buffer: wgpu::Buffer,
    #[cfg(feature = "3d")]
    mesh3d_scene_bind_group: wgpu::BindGroup,
    #[cfg(feature = "3d")]
    mesh3d_object_bgl: wgpu::BindGroupLayout,
    #[cfg(feature = "3d")]
    mesh3d_object_buffers: Vec<wgpu::Buffer>,
    #[cfg(feature = "3d")]
    mesh3d_object_bind_groups: Vec<wgpu::BindGroup>,
    #[cfg(feature = "3d")]
    meshes3d: Vec<GpuMesh>,
    #[cfg(feature = "3d")]
    mesh3d_queue: Vec<QueuedMesh3D>,
    #[cfg(feature = "3d")]
    mesh3d_scene: Scene3DState,
}

fn normalized_scale_factor(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

fn logical_viewport(size: winit::dpi::PhysicalSize<u32>, scale_factor: f64) -> (f32, f32) {
    let scale_factor = normalized_scale_factor(scale_factor);
    (
        (size.width.max(1) as f64 / scale_factor).max(1.0) as f32,
        (size.height.max(1) as f64 / scale_factor).max(1.0) as f32,
    )
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = normalized_scale_factor(window.scale_factor());
        let (logical_width, logical_height) = logical_viewport(size, scale_factor);

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

        #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
        let mut surface_usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        // Only needed natively for `request_screenshot`'s buffer readback;
        // some WebGPU implementations reject COPY_SRC on the swapchain.
        #[cfg(not(target_arch = "wasm32"))]
        {
            surface_usage |= wgpu::TextureUsages::COPY_SRC;
        }

        let config = wgpu::SurfaceConfiguration {
            usage: surface_usage,
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
        let camera = Camera2D::new(logical_width, logical_height);
        let post = PostPipeline::new(&device, width, height, surface_format);

        #[cfg(feature = "3d")]
        let (
            depth_texture,
            depth_view,
            mesh3d_pipeline,
            mesh3d_scene_buffer,
            mesh3d_scene_bind_group,
            mesh3d_object_bgl,
        ) = {
            let (depth_texture, depth_view) = create_depth_target(&device, width, height);

            let mesh3d_scene_bgl =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Mesh3D Scene BGL"),
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
            let mesh3d_scene_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Mesh3D Scene UB"),
                size: std::mem::size_of::<Scene3DUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mesh3d_scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Mesh3D Scene BG"),
                layout: &mesh3d_scene_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mesh3d_scene_buffer.as_entire_binding(),
                }],
            });

            let mesh3d_object_bgl =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Mesh3D Object BGL"),
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

            let mesh3d_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Mesh3D Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/mesh3d.wgsl").into()),
            });
            let mesh3d_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Mesh3D PL"),
                bind_group_layouts: &[&mesh3d_scene_bgl, &mesh3d_object_bgl],
                push_constant_ranges: &[],
            });
            let mesh3d_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Mesh3D Pipeline"),
                layout: Some(&mesh3d_layout),
                vertex: wgpu::VertexState {
                    module: &mesh3d_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[GpuMesh::layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh3d_shader,
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
                    // Double-sided: keeps the core pipeline robust to winding
                    // order across arbitrary future mesh sources; depth testing
                    // still resolves occlusion for closed solids correctly.
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

            (
                depth_texture,
                depth_view,
                mesh3d_pipeline,
                mesh3d_scene_buffer,
                mesh3d_scene_bind_group,
                mesh3d_object_bgl,
            )
        };

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
            #[cfg(not(target_arch = "wasm32"))]
            pending_captures: Vec::new(),
            sprite_pipeline,
            camera_buffer,
            camera_bind_group,
            texture_bind_group_layout,
            sprite_sampler,
            batch,
            textures: Vec::new(),
            draw_queue: Vec::with_capacity(1024),
            light_queue: Vec::with_capacity(MAX_POINT_LIGHTS),
            tri_pipeline,
            tri_vbo,
            tri_uniform,
            tri_bind_group,
            show_debug_triangle: false,
            post,
            post_fx: PostFxSettings::default(),
            quality: RenderQuality::default(),
            camera,
            scale_factor,
            stats: RenderStats::default(),
            window,

            #[cfg(feature = "3d")]
            depth_texture,
            #[cfg(feature = "3d")]
            depth_view,
            #[cfg(feature = "3d")]
            mesh3d_pipeline,
            #[cfg(feature = "3d")]
            mesh3d_scene_buffer,
            #[cfg(feature = "3d")]
            mesh3d_scene_bind_group,
            #[cfg(feature = "3d")]
            mesh3d_object_bgl,
            #[cfg(feature = "3d")]
            mesh3d_object_buffers: Vec::new(),
            #[cfg(feature = "3d")]
            mesh3d_object_bind_groups: Vec::new(),
            #[cfg(feature = "3d")]
            meshes3d: Vec::new(),
            #[cfg(feature = "3d")]
            mesh3d_queue: Vec::new(),
            #[cfg(feature = "3d")]
            mesh3d_scene: Scene3DState::default(),
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

    /// Update the logical viewport after a native DPI/scale-factor change.
    ///
    /// Winit may deliver `ScaleFactorChanged` without an immediate `Resized`
    /// event. Updating the cached scale and camera together keeps pointer
    /// hit-testing, HUD anchors, and world rendering in the same coordinate
    /// space during that gap.
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = normalized_scale_factor(scale_factor);
        let (width, height) = logical_viewport(self.size, self.scale_factor);
        self.camera.set_viewport(width, height);
    }

    pub fn set_clear_color(&mut self, color: Color) {
        self.clear_color = color;
    }

    /// Queues a screenshot: a future `render()` call (one per frame, in
    /// request order) writes the presented frame to `path` as a PNG.
    /// Native only — a debug/dev-tools facility (see
    /// `AURORA_SCREENSHOT_PATH` in `app.rs`), not part of the normal game
    /// loop.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_screenshot(&mut self, path: impl Into<std::path::PathBuf>) {
        self.pending_captures.push(path.into());
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

    /// Queue a radial HDR light for this frame. Lights are automatically
    /// cleared after `render`, matching the sprite queue lifetime.
    pub fn draw_light(&mut self, light: PointLight) {
        if light.radius > 0.0 && light.intensity > 0.0 {
            self.light_queue.push(light);
        }
    }

    /// Selects the portable per-frame point-light budget.
    pub fn set_quality(&mut self, quality: RenderQuality) {
        self.quality = quality;
    }

    pub fn quality(&self) -> RenderQuality {
        self.quality
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Uploads a mesh to the GPU and returns a stable handle to draw it with.
    /// Call once per unique mesh (typically from `Game::on_start`).
    #[cfg(feature = "3d")]
    pub fn upload_mesh3d(&mut self, mesh: &Mesh3D) -> Mesh3DHandle {
        let gpu_mesh = GpuMesh::upload(&self.gpu(), mesh);
        let idx = self.meshes3d.len();
        self.meshes3d.push(gpu_mesh);
        Mesh3DHandle(idx)
    }

    /// Sets the camera used by the mesh pipeline for the next `render` call.
    #[cfg(feature = "3d")]
    pub fn set_camera3d(&mut self, camera: &Camera3D) {
        self.mesh3d_scene.view_proj = camera.view_projection();
        self.mesh3d_scene.camera_pos = camera.position;
    }

    /// Sets the single directional light used by the mesh pipeline.
    #[cfg(feature = "3d")]
    pub fn set_directional_light(&mut self, direction: Vec3, color: Color, intensity: f32) {
        self.mesh3d_scene.light_dir = direction.normalize_or_zero();
        self.mesh3d_scene.light_color = color;
        self.mesh3d_scene.light_intensity = intensity;
    }

    /// Queues a mesh instance for the next frame (call during `on_update`).
    #[cfg(feature = "3d")]
    pub fn queue_mesh3d(&mut self, mesh: Mesh3DHandle, transform: Mat4, material: Material3D) {
        if mesh.0 < self.meshes3d.len() {
            self.mesh3d_queue.push(QueuedMesh3D {
                mesh,
                transform,
                material: material.sanitized(),
            });
        }
    }

    #[cfg(feature = "3d")]
    fn ensure_mesh3d_object_capacity(&mut self, needed: usize) {
        while self.mesh3d_object_buffers.len() < needed {
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Mesh3D Object UB"),
                size: std::mem::size_of::<Object3DUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Mesh3D Object BG"),
                layout: &self.mesh3d_object_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            self.mesh3d_object_buffers.push(buffer);
            self.mesh3d_object_bind_groups.push(bind_group);
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            let (width, height) = logical_viewport(new_size, self.scale_factor);
            self.camera.set_viewport(width, height);
            self.post
                .resize(&self.device, new_size.width, new_size.height);
            #[cfg(feature = "3d")]
            {
                let (depth_texture, depth_view) =
                    create_depth_target(&self.device, new_size.width, new_size.height);
                self.depth_texture = depth_texture;
                self.depth_view = depth_view;
            }
        }
    }

    pub fn render(&mut self, elapsed: f32) -> Result<(), wgpu::SurfaceError> {
        let frame_started = InstantCompat::now();
        let vp: Mat4 = self.camera.view_projection();
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::bytes_of(&camera_uniform(vp)),
        );

        #[cfg(feature = "3d")]
        let mesh3d_queue = std::mem::take(&mut self.mesh3d_queue);
        #[cfg(feature = "3d")]
        {
            let scene = &self.mesh3d_scene;
            let scene_uniform = Scene3DUniform {
                view_proj: scene.view_proj.to_cols_array_2d(),
                camera_pos: [
                    scene.camera_pos.x,
                    scene.camera_pos.y,
                    scene.camera_pos.z,
                    0.0,
                ],
                light_dir: [scene.light_dir.x, scene.light_dir.y, scene.light_dir.z, 0.0],
                light_color: [
                    scene.light_color.r,
                    scene.light_color.g,
                    scene.light_color.b,
                    scene.light_intensity,
                ],
            };
            self.queue.write_buffer(
                &self.mesh3d_scene_buffer,
                0,
                bytemuck::bytes_of(&scene_uniform),
            );

            self.ensure_mesh3d_object_capacity(mesh3d_queue.len());
            for (i, item) in mesh3d_queue.iter().enumerate() {
                let object_uniform = Object3DUniform {
                    model: item.transform.to_cols_array_2d(),
                    base_color: [
                        item.material.base_color.r,
                        item.material.base_color.g,
                        item.material.base_color.b,
                        item.material.base_color.a,
                    ],
                    emissive: [
                        item.material.emissive.r,
                        item.material.emissive.g,
                        item.material.emissive.b,
                        0.0,
                    ],
                    metallic_roughness: [item.material.metallic, item.material.roughness, 0.0, 0.0],
                };
                self.queue.write_buffer(
                    &self.mesh3d_object_buffers[i],
                    0,
                    bytemuck::bytes_of(&object_uniform),
                );
            }
        }

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
        let lights = std::mem::take(&mut self.light_queue);
        let composed_lights = lights.len().min(self.quality.light_budget());
        self.stats = RenderStats {
            queued_sprites: queue.len(),
            drawn_sprites: queue.len(),
            draw_calls: 0,
            queued_lights: lights.len(),
            composed_lights,
            cpu_frame_ms: 0.0,
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
            &lights[..composed_lights]
                .iter()
                .map(|light| self.screen_light(*light))
                .collect::<Vec<_>>(),
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

        // Pass 0 (3d feature only): opaque, depth-tested mesh scene → offscreen
        #[cfg(feature = "3d")]
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Mesh3D Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.clear_color.to_wgpu()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            if !mesh3d_queue.is_empty() {
                pass.set_pipeline(&self.mesh3d_pipeline);
                pass.set_bind_group(0, &self.mesh3d_scene_bind_group, &[]);
                for (i, item) in mesh3d_queue.iter().enumerate() {
                    let Some(mesh) = self.meshes3d.get(item.mesh.0) else {
                        continue;
                    };
                    pass.set_bind_group(1, &self.mesh3d_object_bind_groups[i], &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
        }

        // A mesh pass already cleared+drew the offscreen target when the `3d`
        // feature is enabled, so the sprite scene pass loads instead of
        // clearing — 2D sprites (HUD/UI) composite on top of the 3D scene.
        let scene_color_load = if cfg!(feature = "3d") {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(self.clear_color.to_wgpu())
        };

        // Pass 1: scene → offscreen
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: scene_color_load,
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

        #[cfg(not(target_arch = "wasm32"))]
        let capture = (!self.pending_captures.is_empty())
            .then(|| self.pending_captures.remove(0))
            .map(|path| {
                let bytes_per_pixel = 4u32;
                let unpadded_bytes_per_row = self.config.width * bytes_per_pixel;
                let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Screenshot Buffer"),
                    size: (padded_bytes_per_row * self.config.height) as wgpu::BufferAddress,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: &output.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded_bytes_per_row),
                            rows_per_image: Some(self.config.height),
                        },
                    },
                    wgpu::Extent3d {
                        width: self.config.width,
                        height: self.config.height,
                        depth_or_array_layers: 1,
                    },
                );
                (path, buffer, padded_bytes_per_row, unpadded_bytes_per_row)
            });

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        #[cfg(not(target_arch = "wasm32"))]
        if let Some((path, buffer, padded_bytes_per_row, unpadded_bytes_per_row)) = capture {
            self.write_capture_png(
                path,
                buffer,
                padded_bytes_per_row,
                unpadded_bytes_per_row,
                self.config.format,
            );
        }

        self.stats.cpu_frame_ms = InstantCompat::now()
            .duration_since(frame_started)
            .as_secs_f32()
            * 1_000.0;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn write_capture_png(
        &self,
        path: std::path::PathBuf,
        buffer: wgpu::Buffer,
        padded_bytes_per_row: u32,
        unpadded_bytes_per_row: u32,
        format: wgpu::TextureFormat,
    ) {
        let width = self.config.width;
        let height = self.config.height;
        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        let Ok(Ok(())) = rx.recv() else {
            log::warn!("screenshot buffer map failed");
            return;
        };
        let mut pixels = vec![0u8; (unpadded_bytes_per_row * height) as usize];
        {
            let data = slice.get_mapped_range();
            for row in 0..height {
                let src_start = (row * padded_bytes_per_row) as usize;
                let dst_start = (row * unpadded_bytes_per_row) as usize;
                let row_len = unpadded_bytes_per_row as usize;
                pixels[dst_start..dst_start + row_len]
                    .copy_from_slice(&data[src_start..src_start + row_len]);
            }
        }
        buffer.unmap();
        if matches!(
            format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for chunk in pixels.chunks_exact_mut(4) {
                chunk.swap(0, 2);
            }
        }
        match image::RgbaImage::from_raw(width, height, pixels) {
            Some(image) => match image.save(&path) {
                Ok(()) => log::info!("screenshot saved to {}", path.display()),
                Err(error) => log::warn!("screenshot save failed: {error}"),
            },
            None => log::warn!("screenshot buffer had the wrong size for {width}x{height}"),
        }
    }

    fn screen_light(&self, light: PointLight) -> ScreenLight {
        let screen = self.camera.world_to_screen(light.position);
        let viewport = self.camera.viewport();
        let radius_pixels = light.radius * self.camera.zoom;
        ScreenLight {
            position_uv: [screen.x / viewport.x, screen.y / viewport.y],
            radius_uv: radius_pixels / viewport.y,
            intensity: light.intensity,
            color: [light.color.r, light.color.g, light.color.b],
        }
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
    let uvs = sprite_corner_uvs(sprite.uv_min, sprite.uv_max);
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

#[cfg(test)]
mod tests {
    use super::{logical_viewport, normalized_scale_factor, push_sprite_mesh, RenderQuality};
    use crate::sprite::Sprite;
    use glam::Vec2;
    use winit::dpi::PhysicalSize;

    #[test]
    fn quality_presets_use_bounded_light_budgets() {
        assert!(RenderQuality::Performance.light_budget() < RenderQuality::Balanced.light_budget());
        assert!(RenderQuality::Balanced.light_budget() < RenderQuality::Cinematic.light_budget());
    }

    #[test]
    fn renderer_mesh_maps_world_bottom_to_image_bottom() {
        let mut sprite = Sprite::new(Vec2::ZERO, Vec2::ONE);
        sprite.uv_min = Vec2::new(0.25, 0.10);
        sprite.uv_max = Vec2::new(0.50, 0.40);
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        push_sprite_mesh(&sprite, &mut vertices, &mut indices);

        assert_eq!(vertices[0].uv, [0.25, 0.40]);
        assert_eq!(vertices[1].uv, [0.50, 0.40]);
        assert_eq!(vertices[2].uv, [0.50, 0.10]);
        assert_eq!(vertices[3].uv, [0.25, 0.10]);
    }

    #[test]
    fn logical_viewport_tracks_scale_factor_without_nan_or_zero_dimensions() {
        assert_eq!(
            logical_viewport(PhysicalSize::new(1920, 1080), 2.0),
            (960.0, 540.0)
        );
        assert_eq!(
            logical_viewport(PhysicalSize::new(0, 0), f64::NAN),
            (1.0, 1.0)
        );
        assert_eq!(normalized_scale_factor(0.0), 1.0);
        assert_eq!(normalized_scale_factor(f64::INFINITY), 1.0);
    }
}
