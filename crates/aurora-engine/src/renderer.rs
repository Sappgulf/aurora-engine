//! GPU renderer: clear + multi-texture sprite batches + optional debug triangle.

use std::fmt;
use std::sync::Arc;

#[cfg(feature = "3d")]
use glam::Vec3;
use glam::{Mat4, Vec2};
use serde::{Deserialize, Serialize};
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
use crate::Aabb;

/// Stable handle returned when a texture is registered with the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextureHandle(pub(crate) usize);

/// Per-frame admission limits for renderer work. Limits are intentionally
/// separate for normal sprites, debug sprites, and lights so diagnostics can
/// distinguish a tooling flood from game rendering pressure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderBudget {
    pub max_sprites: usize,
    pub max_debug_sprites: usize,
    pub max_lights: usize,
}

impl Default for RenderBudget {
    fn default() -> Self {
        Self {
            max_sprites: 100_000,
            max_debug_sprites: 20_000,
            max_lights: 256,
        }
    }
}

impl RenderBudget {
    fn normalized(self) -> Self {
        Self {
            max_sprites: self.max_sprites.max(1),
            max_debug_sprites: self.max_debug_sprites.max(1),
            max_lights: self.max_lights.max(1),
        }
    }
}

/// Per-frame render counters for debug HUDs and performance tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub queued_sprites: usize,
    pub invalid_sprites: usize,
    pub drawn_sprites: usize,
    /// Normal sprites rejected by the per-frame admission budget.
    pub dropped_sprites: usize,
    /// Debug sprites rejected by the per-frame admission budget.
    pub dropped_debug_sprites: usize,
    /// Sprites skipped because their bounding circle fell entirely outside
    /// the camera viewport (including its cull margin).
    pub culled_sprites: usize,
    pub draw_calls: usize,
    pub queued_lights: usize,
    pub composed_lights: usize,
    /// Lights rejected by the per-frame admission budget.
    pub dropped_lights: usize,
    /// CPU time spent encoding and presenting the most recent frame.
    pub cpu_frame_ms: f32,
    pub staged_vertices: usize,
    pub staged_indices: usize,
    pub sprite_upload_bytes: usize,
    pub quality: RenderQuality,
}

fn admit_bounded<T>(queue: &mut Vec<T>, item: T, limit: usize, dropped: &mut usize) {
    if queue.len() < limit {
        queue.push(item);
    } else {
        *dropped = dropped.saturating_add(1);
    }
}

fn take_with_capacity<T>(queue: &mut Vec<T>) -> Vec<T> {
    let capacity = queue.capacity();
    std::mem::replace(queue, Vec::with_capacity(capacity))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

/// Directional-light shadow map settings for the Mesh3D pipeline. The shadow
/// region is an ortho box of half-extent [`ShadowSettings::extent`] centered
/// on the world origin.
#[cfg(feature = "3d")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowSettings {
    pub enabled: bool,
    /// Side length of the square depth map in texels (recreated on change).
    pub map_size: u32,
    /// Constant depth bias applied when sampling, scaled by slope.
    pub depth_bias: f32,
    /// Half-extent of the ortho shadow region around the origin, in world units.
    pub extent: f32,
}

#[cfg(feature = "3d")]
impl Default for ShadowSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            map_size: 2048,
            depth_bias: 0.0015,
            extent: 40.0,
        }
    }
}

/// Gradient sky rendered as the Mesh3D background and reused as hemispheric
/// ambient light by the PBR shader.
#[cfg(feature = "3d")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkySettings {
    pub enabled: bool,
    /// HDR color at the top of the sky.
    pub zenith: [f32; 3],
    /// HDR color at (and slightly below) the horizon.
    pub horizon: [f32; 3],
    /// Brightness of the sun disk drawn toward the directional light.
    pub sun_intensity: f32,
}

#[cfg(feature = "3d")]
impl Default for SkySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            zenith: [0.22, 0.42, 0.85],
            horizon: [0.78, 0.86, 0.94],
            sun_intensity: 1.0,
        }
    }
}

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
    inv_view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 4],
    light_dir: [f32; 4],
    light_color: [f32; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    // x = shadows enabled, y = depth bias, z = shadow texel size
    shadow_params: [f32; 4],
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

#[cfg(feature = "3d")]
fn create_shadow_target(device: &wgpu::Device, size: u32) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Mesh3D Shadow Map"),
        size: wgpu::Extent3d {
            width: size.max(1),
            height: size.max(1),
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
/// One batched texture run within a composed frame.
struct DrawRange {
    texture: TextureHandle,
    index_start: u32,
    index_count: u32,
}

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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    camera_bgl: wgpu::BindGroupLayout,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    sprite_sampler: wgpu::Sampler,
    batch: SpriteBatch,
    textures: Vec<Texture>,
    draw_queue: Vec<QueuedSprite>,
    light_queue: Vec<PointLight>,
    budget: RenderBudget,
    dropped_sprites: usize,
    dropped_debug_sprites: usize,
    dropped_lights: usize,
    // Debug shapes queued through `draw_debug_*`. Cleared by `render`, drawn
    // after normal sprites in the scene pass.
    debug_queue: Vec<QueuedSprite>,
    /// Lazily uploaded 1x1 white texture debug shapes are tinted through.
    debug_texture: Option<TextureHandle>,
    // Frame staging buffers. Cleared every render; capacity is retained so a
    // steady-state frame performs zero staging allocations.
    stage_vertices: Vec<crate::sprite::SpriteVertex>,
    stage_indices: Vec<u32>,
    stage_ranges: Vec<DrawRange>,
    screen_lights: Vec<ScreenLight>,

    // Debug triangle (NDC)
    tri_pipeline: wgpu::RenderPipeline,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    tri_bgl: wgpu::BindGroupLayout,
    tri_vbo: wgpu::Buffer,
    tri_uniform: wgpu::Buffer,
    tri_bind_group: wgpu::BindGroup,
    show_debug_triangle: bool,

    post: PostPipeline,
    /// Full-screen post effects (bloom, vignette, chromatic).
    pub post_fx: PostFxSettings,
    quality: RenderQuality,
    render_scale: f32,
    /// Optional directory consulted by [`Renderer::reload_shaders`] for WGSL
    /// overrides; `None` means "embedded shaders only".
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    shader_dir: Option<std::path::PathBuf>,

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
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    mesh3d_scene_bgl: wgpu::BindGroupLayout,
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
    // Directional-light shadow map (depth-only pass + comparison sampling)
    #[cfg(feature = "3d")]
    #[allow(dead_code)]
    shadow_texture: wgpu::Texture,
    #[cfg(feature = "3d")]
    shadow_view: wgpu::TextureView,
    #[cfg(feature = "3d")]
    shadow_comparison_sampler: wgpu::Sampler,
    #[cfg(feature = "3d")]
    shadow_bind_group: wgpu::BindGroup,
    #[cfg(feature = "3d")]
    mesh3d_shadow_bgl: wgpu::BindGroupLayout,
    #[cfg(feature = "3d")]
    mesh3d_shadow_pipeline: wgpu::RenderPipeline,
    #[cfg(feature = "3d")]
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    mesh3d_light_bgl: wgpu::BindGroupLayout,
    #[cfg(feature = "3d")]
    mesh3d_light_buffer: wgpu::Buffer,
    #[cfg(feature = "3d")]
    mesh3d_light_bind_group: wgpu::BindGroup,
    // Gradient sky background pass
    #[cfg(feature = "3d")]
    mesh3d_sky_pipeline: wgpu::RenderPipeline,
    #[cfg(feature = "3d")]
    shadow: ShadowSettings,
    #[cfg(feature = "3d")]
    shadow_map_size: u32,
    #[cfg(feature = "3d")]
    sky: SkySettings,
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

/// HDR scene color format shared by every scene-pass pipeline.
#[cfg(not(target_arch = "wasm32"))]
const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Thickness, in world units, of debug outlines, rays, and grid lines.
const DEBUG_LINE_THICKNESS: f32 = 2.0;
/// Fixed z for debug shapes so they composite above normal sprite content
/// while staying inside the camera's ±1000 depth range.
const DEBUG_SPRITE_Z: f32 = 900.0;
/// Upper bound on debug grid lines per axis so a pathological spacing cannot
/// flood the debug queue in a single call.
const MAX_DEBUG_GRID_LINES_PER_AXIS: usize = 1024;
/// World-space slack added around the camera viewport during sprite culling
/// so edge-kissing sprites survive rounding error.
const SPRITE_CULL_MARGIN: f32 = 16.0;

/// Conservative circle-versus-viewport test for sprite culling.
///
/// `radius` should be the sprite's half-diagonal so a rotated sprite is only
/// culled once no corner could still touch the view. `view` is the visible
/// world size centered on `camera_center`; `margin` inflates the viewport so
/// sprites kissing the edge survive float error. Returns `true` when the
/// sprite may be visible and must be drawn.
pub fn sprite_in_view(
    center: Vec2,
    radius: f32,
    camera_center: Vec2,
    view: Vec2,
    margin: f32,
) -> bool {
    let half = view * 0.5 + Vec2::splat(margin.max(0.0));
    let radius = radius.max(0.0);
    let delta = center - camera_center;
    let closest = delta.clamp(-half, half);
    (delta - closest).length_squared() <= radius * radius
}

/// Rebuilds the sprite pipeline from `source` (shader hot reload).
#[cfg(not(target_arch = "wasm32"))]
fn build_sprite_pipeline(
    device: &wgpu::Device,
    camera_bgl: &wgpu::BindGroupLayout,
    texture_bgl: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Sprite Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Sprite PL"),
        bind_group_layouts: &[camera_bgl, texture_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Sprite Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[SpriteVertex::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: SCENE_FORMAT,
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
    })
}

/// Rebuilds the debug-triangle pipeline from `source` (shader hot reload).
#[cfg(not(target_arch = "wasm32"))]
fn build_triangle_pipeline(
    device: &wgpu::Device,
    tri_bgl: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Triangle Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Tri PL"),
        bind_group_layouts: &[tri_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Tri Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 20,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x3],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: SCENE_FORMAT,
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
    })
}

/// Rebuilds the PBR mesh pipeline from `source` (shader hot reload).
#[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
fn build_mesh3d_pipeline(
    device: &wgpu::Device,
    scene_bgl: &wgpu::BindGroupLayout,
    object_bgl: &wgpu::BindGroupLayout,
    shadow_bgl: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Mesh3D Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Mesh3D PL"),
        bind_group_layouts: &[scene_bgl, object_bgl, shadow_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Mesh3D Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[GpuMesh::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: SCENE_FORMAT,
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
    })
}

/// Rebuilds the depth-only shadow pipeline from `source` (shader hot reload).
#[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
fn build_shadow_pipeline(
    device: &wgpu::Device,
    light_bgl: &wgpu::BindGroupLayout,
    object_bgl: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Mesh3D Shadow Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Mesh3D Shadow PL"),
        bind_group_layouts: &[light_bgl, object_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Mesh3D Shadow Pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[GpuMesh::layout()],
            compilation_options: Default::default(),
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Front),
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
    })
}

/// Rebuilds the gradient-sky pipeline from `source` (shader hot reload).
#[cfg(all(feature = "3d", not(target_arch = "wasm32")))]
fn build_sky_pipeline(
    device: &wgpu::Device,
    scene_bgl: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Mesh3D Sky Shader"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Mesh3D Sky PL"),
        bind_group_layouts: &[scene_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Mesh3D Sky Pipeline"),
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
                format: SCENE_FORMAT,
                blend: None,
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

#[derive(Debug)]
pub enum RendererInitError {
    SurfaceCreate(String),
    AdapterUnavailable,
    RequestDevice(String),
    MissingSurfaceFormat,
    MissingPresentMode,
    MissingAlphaMode,
}

impl fmt::Display for RendererInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SurfaceCreate(error) => write!(f, "failed to create WGPU surface: {error}"),
            Self::AdapterUnavailable => write!(f, "no compatible GPU adapter found"),
            Self::RequestDevice(error) => write!(f, "failed to create WGPU device: {error}"),
            Self::MissingSurfaceFormat => write!(f, "device exposed no supported surface format"),
            Self::MissingPresentMode => write!(f, "device exposed no supported present mode"),
            Self::MissingAlphaMode => write!(f, "device exposed no supported alpha mode"),
        }
    }
}

impl std::error::Error for RendererInitError {}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, RendererInitError> {
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
            .map_err(|error| RendererInitError::SurfaceCreate(error.to_string()))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or(RendererInitError::AdapterUnavailable)?;

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
            .map_err(|error| RendererInitError::RequestDevice(error.to_string()))?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or(RendererInitError::MissingSurfaceFormat)?;
        // FIFO is the portable VSync mode. Selecting it deliberately keeps
        // Aurora's 60 Hz simulation from presenting with unstable pacing.
        let present_mode = surface_caps
            .present_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::PresentMode::Fifo)
            .or_else(|| surface_caps.present_modes.first().copied())
            .ok_or(RendererInitError::MissingPresentMode)?;
        let alpha_mode = surface_caps
            .alpha_modes
            .first()
            .copied()
            .ok_or(RendererInitError::MissingAlphaMode)?;

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
            alpha_mode,
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
            mesh3d_scene_bgl,
            mesh3d_scene_buffer,
            mesh3d_scene_bind_group,
            mesh3d_object_bgl,
            shadow_texture,
            shadow_view,
            shadow_comparison_sampler,
            shadow_bind_group,
            mesh3d_shadow_bgl,
            mesh3d_shadow_pipeline,
            mesh3d_light_bgl,
            mesh3d_light_buffer,
            mesh3d_light_bind_group,
            mesh3d_sky_pipeline,
            shadow_map_size,
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

            let shadow_map_size = ShadowSettings::default().map_size;
            let (shadow_texture, shadow_view) = create_shadow_target(&device, shadow_map_size);
            let shadow_comparison_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Mesh3D Shadow Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                compare: Some(wgpu::CompareFunction::Less),
                ..Default::default()
            });

            let mesh3d_shadow_bgl =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Mesh3D Shadow BGL"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Depth,
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                            count: None,
                        },
                    ],
                });
            let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Mesh3D Shadow BG"),
                layout: &mesh3d_shadow_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&shadow_comparison_sampler),
                    },
                ],
            });

            let mesh3d_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Mesh3D Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/mesh3d.wgsl").into()),
            });
            let mesh3d_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Mesh3D PL"),
                bind_group_layouts: &[&mesh3d_scene_bgl, &mesh3d_object_bgl, &mesh3d_shadow_bgl],
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

            let mesh3d_light_bgl =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Mesh3D Light BGL"),
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
            let mesh3d_light_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Mesh3D Light UB"),
                size: std::mem::size_of::<Mat4>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mesh3d_light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Mesh3D Light BG"),
                layout: &mesh3d_light_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: mesh3d_light_buffer.as_entire_binding(),
                }],
            });

            let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Mesh3D Shadow Shader"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("../shaders/mesh3d_shadow.wgsl").into(),
                ),
            });
            let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Mesh3D Shadow PL"),
                bind_group_layouts: &[&mesh3d_light_bgl, &mesh3d_object_bgl],
                push_constant_ranges: &[],
            });
            let mesh3d_shadow_pipeline =
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Mesh3D Shadow Pipeline"),
                    layout: Some(&shadow_layout),
                    vertex: wgpu::VertexState {
                        module: &shadow_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[GpuMesh::layout()],
                        compilation_options: Default::default(),
                    },
                    fragment: None,
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        // Store back-face depth so closed receivers cannot
                        // self-shadow; the bias covers curved/seamed edges.
                        cull_mode: Some(wgpu::Face::Front),
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

            let sky_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Mesh3D Sky Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/mesh3d_sky.wgsl").into()),
            });
            let sky_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Mesh3D Sky PL"),
                bind_group_layouts: &[&mesh3d_scene_bgl],
                push_constant_ranges: &[],
            });
            let mesh3d_sky_pipeline =
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Mesh3D Sky Pipeline"),
                    layout: Some(&sky_layout),
                    vertex: wgpu::VertexState {
                        module: &sky_shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &sky_shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: scene_format,
                            blend: None,
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
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: false,
                        depth_compare: wgpu::CompareFunction::Always,
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
                mesh3d_scene_bgl,
                mesh3d_scene_buffer,
                mesh3d_scene_bind_group,
                mesh3d_object_bgl,
                shadow_texture,
                shadow_view,
                shadow_comparison_sampler,
                shadow_bind_group,
                mesh3d_shadow_bgl,
                mesh3d_shadow_pipeline,
                mesh3d_light_bgl,
                mesh3d_light_buffer,
                mesh3d_light_bind_group,
                mesh3d_sky_pipeline,
                shadow_map_size,
            )
        };

        log::info!(
            "Aurora renderer ready — adapter: {:?}, format: {:?}",
            adapter.get_info().name,
            surface_format
        );

        Ok(Self {
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
            camera_bgl,
            texture_bind_group_layout,
            sprite_sampler,
            batch,
            textures: Vec::new(),
            draw_queue: Vec::with_capacity(1024),
            light_queue: Vec::with_capacity(MAX_POINT_LIGHTS),
            budget: RenderBudget::default().normalized(),
            dropped_sprites: 0,
            dropped_debug_sprites: 0,
            dropped_lights: 0,
            debug_queue: Vec::with_capacity(256),
            debug_texture: None,
            stage_vertices: Vec::with_capacity(4096),
            stage_indices: Vec::with_capacity(6144),
            stage_ranges: Vec::with_capacity(64),
            screen_lights: Vec::new(),
            tri_pipeline,
            tri_bgl,
            tri_vbo,
            tri_uniform,
            tri_bind_group,
            show_debug_triangle: false,
            post,
            post_fx: PostFxSettings::default(),
            quality: RenderQuality::default(),
            render_scale: 1.0,
            shader_dir: None,
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
            mesh3d_scene_bgl,
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
            #[cfg(feature = "3d")]
            shadow_texture,
            #[cfg(feature = "3d")]
            shadow_view,
            #[cfg(feature = "3d")]
            shadow_comparison_sampler,
            #[cfg(feature = "3d")]
            shadow_bind_group,
            #[cfg(feature = "3d")]
            mesh3d_shadow_bgl,
            #[cfg(feature = "3d")]
            mesh3d_shadow_pipeline,
            #[cfg(feature = "3d")]
            mesh3d_light_bgl,
            #[cfg(feature = "3d")]
            mesh3d_light_buffer,
            #[cfg(feature = "3d")]
            mesh3d_light_bind_group,
            #[cfg(feature = "3d")]
            mesh3d_sky_pipeline,
            #[cfg(feature = "3d")]
            shadow: ShadowSettings::default(),
            #[cfg(feature = "3d")]
            shadow_map_size,
            #[cfg(feature = "3d")]
            sky: SkySettings::default(),
        })
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

    /// Sets per-frame queue admission limits. Lowering a limit immediately
    /// trims already queued work so the new bound is true for the current
    /// frame as well as future frames.
    pub fn set_budget(&mut self, budget: RenderBudget) {
        let budget = budget.normalized();
        let dropped_sprites = self.draw_queue.len().saturating_sub(budget.max_sprites);
        if dropped_sprites > 0 {
            self.draw_queue.truncate(budget.max_sprites);
            self.dropped_sprites = self.dropped_sprites.saturating_add(dropped_sprites);
        }
        let dropped_debug_sprites = self
            .debug_queue
            .len()
            .saturating_sub(budget.max_debug_sprites);
        if dropped_debug_sprites > 0 {
            self.debug_queue.truncate(budget.max_debug_sprites);
            self.dropped_debug_sprites = self
                .dropped_debug_sprites
                .saturating_add(dropped_debug_sprites);
        }
        let dropped_lights = self.light_queue.len().saturating_sub(budget.max_lights);
        if dropped_lights > 0 {
            self.light_queue.truncate(budget.max_lights);
            self.dropped_lights = self.dropped_lights.saturating_add(dropped_lights);
        }
        self.budget = budget;
    }

    pub fn budget(&self) -> RenderBudget {
        self.budget
    }

    /// Queue a sprite for the next frame (call during `on_update`).
    pub fn draw_sprite(&mut self, texture: TextureHandle, sprite: Sprite) {
        admit_bounded(
            &mut self.draw_queue,
            QueuedSprite { texture, sprite },
            self.budget.max_sprites,
            &mut self.dropped_sprites,
        );
    }

    /// Queue a radial HDR light for this frame. Lights are automatically
    /// cleared after `render`, matching the sprite queue lifetime.
    pub fn draw_light(&mut self, light: PointLight) {
        if light.radius > 0.0 && light.intensity > 0.0 {
            admit_bounded(
                &mut self.light_queue,
                light,
                self.budget.max_lights,
                &mut self.dropped_lights,
            );
        }
    }

    /// Queue a solid-color axis-aligned outline for this frame. Debug shapes
    /// reuse the sprite path, draw after normal sprites at a fixed high z,
    /// and are cleared by `render` like the sprite queue.
    pub fn draw_debug_aabb(&mut self, aabb: Aabb, color: Color) {
        for (position, edge_size) in debug_aabb_edges(aabb) {
            self.queue_debug_sprite(
                Sprite::new(position, edge_size)
                    .with_color(color)
                    .with_z(DEBUG_SPRITE_Z),
            );
        }
    }

    /// Queue a solid-color ray for this frame: a thin rect starting at
    /// `origin` and running along `direction`, with length equal to the
    /// direction magnitude. Zero-length rays draw nothing.
    pub fn draw_debug_ray(&mut self, origin: Vec2, direction: Vec2) {
        let Some((position, size, rotation)) = debug_ray_rect(origin, direction) else {
            return;
        };
        self.queue_debug_sprite(
            Sprite::new(position, size)
                .with_rotation(rotation)
                .with_z(DEBUG_SPRITE_Z),
        );
    }

    /// Queue a solid-color world grid for this frame: vertical and
    /// horizontal lines every `spacing` world units across `bounds`. Lines
    /// per axis are capped so a degenerate spacing cannot flood the queue.
    pub fn draw_debug_grid(&mut self, spacing: f32, bounds: Aabb, color: Color) {
        if spacing <= 0.0 {
            return;
        }
        let thickness = DEBUG_LINE_THICKNESS;
        let size = bounds.size();
        let center = bounds.center();

        let mut columns = 0usize;
        let mut x = bounds.min.x;
        while x <= bounds.max.x && columns < MAX_DEBUG_GRID_LINES_PER_AXIS {
            self.queue_debug_sprite(
                Sprite::new(Vec2::new(x, center.y), Vec2::new(thickness, size.y))
                    .with_color(color)
                    .with_z(DEBUG_SPRITE_Z),
            );
            columns += 1;
            x += spacing;
        }

        let mut rows = 0usize;
        let mut y = bounds.min.y;
        while y <= bounds.max.y && rows < MAX_DEBUG_GRID_LINES_PER_AXIS {
            self.queue_debug_sprite(
                Sprite::new(Vec2::new(center.x, y), Vec2::new(size.x, thickness))
                    .with_color(color)
                    .with_z(DEBUG_SPRITE_Z),
            );
            rows += 1;
            y += spacing;
        }
    }

    fn queue_debug_sprite(&mut self, sprite: Sprite) {
        if self.debug_queue.len() >= self.budget.max_debug_sprites {
            self.dropped_debug_sprites = self.dropped_debug_sprites.saturating_add(1);
            return;
        }
        let texture = self.debug_texture_handle();
        self.debug_queue.push(QueuedSprite { texture, sprite });
    }

    /// Lazily uploads the 1x1 white texture debug shapes are tinted through.
    fn debug_texture_handle(&mut self) -> TextureHandle {
        if let Some(handle) = self.debug_texture {
            return handle;
        }
        let handle = self.add_texture(Texture::solid(&self.gpu(), Color::WHITE));
        self.debug_texture = Some(handle);
        handle
    }

    /// Selects the portable per-frame point-light budget.
    pub fn set_quality(&mut self, quality: RenderQuality) {
        self.quality = quality;
    }

    pub fn quality(&self) -> RenderQuality {
        self.quality
    }

    /// Sets the presentation render scale intent. The current renderer keeps
    /// the swapchain size stable; a later presentation path may use this
    /// normalized value for an internal render target.
    pub fn set_render_scale(&mut self, scale: f32) {
        self.render_scale = if scale.is_finite() {
            scale.clamp(0.5, 1.0)
        } else {
            1.0
        };
    }

    pub fn render_scale(&self) -> f32 {
        self.render_scale
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

    /// Configures the directional-light shadow map. Changing `map_size`
    /// recreates the shadow depth texture.
    #[cfg(feature = "3d")]
    pub fn set_shadow_settings(&mut self, settings: ShadowSettings) {
        self.shadow = settings;
        let map_size = settings.map_size.max(1);
        if map_size != self.shadow_map_size {
            let (shadow_texture, shadow_view) = create_shadow_target(&self.device, map_size);
            self.shadow_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Mesh3D Shadow BG"),
                layout: &self.mesh3d_shadow_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.shadow_comparison_sampler),
                    },
                ],
            });
            self.shadow_texture = shadow_texture;
            self.shadow_view = shadow_view;
            self.shadow_map_size = map_size;
        }
    }

    #[cfg(feature = "3d")]
    pub fn shadow_settings(&self) -> ShadowSettings {
        self.shadow
    }

    #[cfg(feature = "3d")]
    pub fn set_sky_settings(&mut self, settings: SkySettings) {
        self.sky = settings;
    }

    #[cfg(feature = "3d")]
    pub fn sky_settings(&self) -> SkySettings {
        self.sky
    }

    /// Points [`Renderer::reload_shaders`] at a directory of WGSL overrides.
    /// Each file name must match the embedded shader (`sprite.wgsl`,
    /// `post.wgsl`, `triangle.wgsl`, plus `mesh3d*.wgsl` under the `3d`
    /// feature); files that are missing fall back to the embedded source.
    /// Native only — on WASM this is a no-op.
    pub fn set_shader_dir(&mut self, dir: Option<std::path::PathBuf>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shader_dir = dir;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = dir;
        }
    }

    pub fn shader_dir(&self) -> Option<&std::path::Path> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shader_dir.as_deref()
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Rebuilds every render pipeline from the sources in [`Renderer::shader_dir`]
    /// (embedded source where no override exists).
    ///
    /// Uniform buffers, bind groups, and render targets are untouched, so
    /// existing textures and meshes stay valid across a reload. WGSL that
    /// fails validation keeps the previous pipeline and is reported in the
    /// returned error list — a failed reload never takes the renderer down.
    /// Native only; on WASM this returns a single Unsupported error.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reload_shaders(&mut self) -> Result<(), Vec<String>> {
        let mut errors: Vec<String> = Vec::new();

        let source = |name: &str, embedded: &'static str| -> String {
            match self.shader_dir.as_ref() {
                Some(dir) => std::fs::read_to_string(dir.join(name)).unwrap_or_else(|error| {
                    log::info!(
                        "shader hot reload: {name} not overridden ({error}); using embedded"
                    );
                    embedded.to_owned()
                }),
                None => embedded.to_owned(),
            }
        };

        // Each rebuild runs inside a validation error scope so broken WGSL is
        // captured as a value instead of panicking the device.
        fn build_validated(
            device: &wgpu::Device,
            label: &str,
            build: impl FnOnce() -> wgpu::RenderPipeline,
        ) -> Result<wgpu::RenderPipeline, String> {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let candidate = build();
            match pollster::block_on(device.pop_error_scope()) {
                Some(error) => Err(format!("{label}: {error}")),
                None => Ok(candidate),
            }
        }

        let camera_bgl = self.camera_bgl.clone();
        let texture_bgl = self.texture_bind_group_layout.clone();
        let sprite_source = source("sprite.wgsl", include_str!("../shaders/sprite.wgsl"));
        match build_validated(&self.device, "sprite.wgsl", || {
            build_sprite_pipeline(&self.device, &camera_bgl, &texture_bgl, &sprite_source)
        }) {
            Ok(pipeline) => self.sprite_pipeline = pipeline,
            Err(error) => errors.push(error),
        }

        let tri_bgl = self.tri_bgl.clone();
        let tri_source = source("triangle.wgsl", include_str!("../shaders/triangle.wgsl"));
        match build_validated(&self.device, "triangle.wgsl", || {
            build_triangle_pipeline(&self.device, &tri_bgl, &tri_source)
        }) {
            Ok(pipeline) => self.tri_pipeline = pipeline,
            Err(error) => errors.push(error),
        }

        let post_bgl = self.post.bind_group_layout.clone();
        let surface_format = self.config.format;
        let post_source = source("post.wgsl", include_str!("../shaders/post.wgsl"));
        match build_validated(&self.device, "post.wgsl", || {
            crate::post::build_post_pipeline(&self.device, &post_bgl, surface_format, &post_source)
        }) {
            Ok(pipeline) => self.post.pipeline = pipeline,
            Err(error) => errors.push(error),
        }

        #[cfg(feature = "3d")]
        {
            let scene_bgl = self.mesh3d_scene_bgl.clone();
            let object_bgl = self.mesh3d_object_bgl.clone();
            let shadow_bgl = self.mesh3d_shadow_bgl.clone();
            let mesh_source = source("mesh3d.wgsl", include_str!("../shaders/mesh3d.wgsl"));
            match build_validated(&self.device, "mesh3d.wgsl", || {
                build_mesh3d_pipeline(
                    &self.device,
                    &scene_bgl,
                    &object_bgl,
                    &shadow_bgl,
                    &mesh_source,
                )
            }) {
                Ok(pipeline) => self.mesh3d_pipeline = pipeline,
                Err(error) => errors.push(error),
            }

            let light_bgl = self.mesh3d_light_bgl.clone();
            let shadow_source = source(
                "mesh3d_shadow.wgsl",
                include_str!("../shaders/mesh3d_shadow.wgsl"),
            );
            match build_validated(&self.device, "mesh3d_shadow.wgsl", || {
                build_shadow_pipeline(&self.device, &light_bgl, &object_bgl, &shadow_source)
            }) {
                Ok(pipeline) => self.mesh3d_shadow_pipeline = pipeline,
                Err(error) => errors.push(error),
            }

            let sky_source = source(
                "mesh3d_sky.wgsl",
                include_str!("../shaders/mesh3d_sky.wgsl"),
            );
            match build_validated(&self.device, "mesh3d_sky.wgsl", || {
                build_sky_pipeline(&self.device, &scene_bgl, &sky_source)
            }) {
                Ok(pipeline) => self.mesh3d_sky_pipeline = pipeline,
                Err(error) => errors.push(error),
            }
        }

        if errors.is_empty() {
            log::info!("shader hot reload applied");
            Ok(())
        } else {
            for error in &errors {
                log::warn!("shader hot reload kept previous pipeline: {error}");
            }
            Err(errors)
        }
    }

    /// Ortho light-space projection covering a half-`extent` box around the
    /// origin, used both for the shadow depth pass and bias sampling.
    #[cfg(feature = "3d")]
    fn light_view_projection(&self) -> Mat4 {
        let direction = self.mesh3d_scene.light_dir;
        let direction = if direction.length_squared() > 1e-8 {
            direction
        } else {
            Vec3::new(-0.4, -1.0, -0.3)
        }
        .normalize_or_zero();
        let center = Vec3::ZERO;
        let eye = center - direction * (self.shadow.extent * 2.0);
        let up = if direction.y.abs() > 0.99 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let half = self.shadow.extent;
        let view = Mat4::look_at_rh(eye, center, up);
        let projection =
            Mat4::orthographic_rh(-half, half, -half, half, 0.1, self.shadow.extent * 4.0);
        projection * view
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
        let shadows_active = self.shadow.enabled && !mesh3d_queue.is_empty();
        #[cfg(feature = "3d")]
        {
            let scene = &self.mesh3d_scene;
            let scene_uniform = Scene3DUniform {
                view_proj: scene.view_proj.to_cols_array_2d(),
                inv_view_proj: scene.view_proj.inverse().to_cols_array_2d(),
                light_view_proj: if shadows_active {
                    self.light_view_projection().to_cols_array_2d()
                } else {
                    Mat4::IDENTITY.to_cols_array_2d()
                },
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
                sky_zenith: [
                    self.sky.zenith[0],
                    self.sky.zenith[1],
                    self.sky.zenith[2],
                    self.sky.sun_intensity,
                ],
                sky_horizon: [
                    self.sky.horizon[0],
                    self.sky.horizon[1],
                    self.sky.horizon[2],
                    0.0,
                ],
                shadow_params: [
                    if shadows_active { 1.0 } else { 0.0 },
                    self.shadow.depth_bias,
                    1.0 / self.shadow.map_size.max(1) as f32,
                    0.0,
                ],
            };
            self.queue.write_buffer(
                &self.mesh3d_scene_buffer,
                0,
                bytemuck::bytes_of(&scene_uniform),
            );

            if shadows_active {
                let light_matrix = self.light_view_projection().to_cols_array_2d();
                self.queue.write_buffer(
                    &self.mesh3d_light_buffer,
                    0,
                    bytemuck::bytes_of(&light_matrix),
                );
            }

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
        let mut queue = take_with_capacity(&mut self.draw_queue);
        let mut lights = take_with_capacity(&mut self.light_queue);
        let composed_lights = lights.len().min(self.quality.light_budget());
        self.stats = RenderStats {
            queued_sprites: queue.len(),
            drawn_sprites: 0,
            invalid_sprites: 0,
            dropped_sprites: self.dropped_sprites,
            dropped_debug_sprites: self.dropped_debug_sprites,
            culled_sprites: 0,
            draw_calls: 0,
            queued_lights: lights.len(),
            composed_lights,
            dropped_lights: self.dropped_lights,
            cpu_frame_ms: 0.0,
            staged_vertices: 0,
            staged_indices: 0,
            sprite_upload_bytes: 0,
            quality: self.quality,
        };
        self.dropped_sprites = 0;
        self.dropped_debug_sprites = 0;
        self.dropped_lights = 0;
        self.batch
            .ensure_capacity(&self.device, queue.len() + self.debug_queue.len());

        self.stage_vertices.clear();
        self.stage_indices.clear();
        self.stage_ranges.clear();
        {
            let view = self.camera.visible_world_size();
            let camera_center = self.camera.position;
            let mut i = 0;
            while i < queue.len() {
                let tex = queue[i].texture;
                let index_start = self.stage_indices.len() as u32;
                let mut local_sprites = 0u32;
                while i < queue.len() && queue[i].texture == tex {
                    if tex.0 < self.textures.len() {
                        // Half-diagonal radius: a rotated sprite stays alive
                        // while any corner could still touch the viewport.
                        let sprite = &queue[i].sprite;
                        let radius = (sprite.size * 0.5).length();
                        if sprite_in_view(
                            sprite.position,
                            radius,
                            camera_center,
                            view,
                            SPRITE_CULL_MARGIN,
                        ) {
                            push_sprite_mesh(
                                sprite,
                                &mut self.stage_vertices,
                                &mut self.stage_indices,
                            );
                            local_sprites += 1;
                        } else {
                            self.stats.culled_sprites += 1;
                        }
                    } else {
                        self.stats.invalid_sprites += 1;
                    }
                    i += 1;
                }
                self.stats.drawn_sprites += local_sprites as usize;

                let index_count = self.stage_indices.len() as u32 - index_start;
                if index_count > 0 {
                    self.stats.draw_calls += 1;
                    self.stage_ranges.push(DrawRange {
                        texture: tex,
                        index_start,
                        index_count,
                    });
                }
            }
        }

        self.stats.staged_vertices = self.stats.drawn_sprites.saturating_mul(4);
        self.stats.staged_indices = self.stats.drawn_sprites.saturating_mul(6);

        // Debug shapes stage after normal sprites so their draw ranges render
        // last in the scene pass; an empty queue stages nothing at all.
        stage_debug_queue(
            &mut self.debug_queue,
            &mut self.stage_vertices,
            &mut self.stage_indices,
            &mut self.stage_ranges,
        );

        let vertex_bytes = self
            .stage_vertices
            .len()
            .saturating_mul(std::mem::size_of::<SpriteVertex>());
        let index_bytes = self
            .stage_indices
            .len()
            .saturating_mul(std::mem::size_of::<u32>());
        self.stats.sprite_upload_bytes = vertex_bytes.saturating_add(index_bytes);

        if !self.stage_vertices.is_empty() {
            self.queue.write_buffer(
                self.batch.vertex_buffer(),
                0,
                bytemuck::cast_slice(&self.stage_vertices),
            );
            self.queue.write_buffer(
                self.batch.index_buffer(),
                0,
                bytemuck::cast_slice(&self.stage_indices),
            );
        }

        self.screen_lights.clear();
        for light in lights[..composed_lights].iter() {
            let mapped = self.screen_light(*light);
            self.screen_lights.push(mapped);
        }
        let post_u = PostUniforms::from_settings(
            &self.post_fx,
            elapsed,
            self.config.width,
            self.config.height,
            &self.screen_lights[..],
        );
        self.queue
            .write_buffer(&self.post.uniform_buffer, 0, bytemuck::bytes_of(&post_u));

        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(error) => {
                queue.clear();
                lights.clear();
                self.draw_queue = queue;
                self.light_queue = lights;
                return Err(error);
            }
        };
        queue.clear();
        lights.clear();
        self.draw_queue = queue;
        self.light_queue = lights;
        let surface_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Pass 0 (3d feature only): shadow depth, then opaque mesh scene → offscreen
        #[cfg(feature = "3d")]
        if shadows_active {
            let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Mesh3D Shadow Pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            shadow_pass.set_pipeline(&self.mesh3d_shadow_pipeline);
            shadow_pass.set_bind_group(0, &self.mesh3d_light_bind_group, &[]);
            for (i, item) in mesh3d_queue.iter().enumerate() {
                let Some(mesh) = self.meshes3d.get(item.mesh.0) else {
                    continue;
                };
                shadow_pass.set_bind_group(1, &self.mesh3d_object_bind_groups[i], &[]);
                shadow_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                shadow_pass
                    .set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                shadow_pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

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

            if self.sky.enabled {
                pass.set_pipeline(&self.mesh3d_sky_pipeline);
                pass.set_bind_group(0, &self.mesh3d_scene_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            if !mesh3d_queue.is_empty() {
                pass.set_pipeline(&self.mesh3d_pipeline);
                pass.set_bind_group(0, &self.mesh3d_scene_bind_group, &[]);
                pass.set_bind_group(2, &self.shadow_bind_group, &[]);
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

            if !self.stage_ranges.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_bind_group(0, &self.camera_bind_group, &[]);
                pass.set_vertex_buffer(0, self.batch.vertex_buffer().slice(..));
                pass.set_index_buffer(
                    self.batch.index_buffer().slice(..),
                    wgpu::IndexFormat::Uint32,
                );

                for range in self.stage_ranges.iter() {
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
            .and_then(|path| {
                let bytes_per_pixel = 4u32;
                let unpadded_bytes_per_row = self.config.width * bytes_per_pixel;
                let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
                let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
                let Some(buffer_size) = u64::from(padded_bytes_per_row)
                    .checked_mul(u64::from(self.config.height))
                    .and_then(|bytes| wgpu::BufferAddress::try_from(bytes).ok())
                else {
                    log::warn!("screenshot capture aborted: pixel buffer size overflow");
                    return None;
                };
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Screenshot Buffer"),
                    size: buffer_size,
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
                Some((path, buffer, padded_bytes_per_row, unpadded_bytes_per_row))
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
        let Some(height_rows) = usize::try_from(height).ok() else {
            log::warn!("screenshot write skipped: invalid capture height");
            return;
        };
        let Some(padded_row_bytes) = usize::try_from(padded_bytes_per_row).ok() else {
            log::warn!("screenshot write skipped: invalid padded row stride");
            return;
        };
        let Some(unpadded_row_bytes) = usize::try_from(unpadded_bytes_per_row).ok() else {
            log::warn!("screenshot write skipped: invalid unpadded row stride");
            return;
        };
        let Some(pixel_buffer_size) = unpadded_row_bytes.checked_mul(height_rows) else {
            log::warn!("screenshot write skipped: row buffer size overflow");
            return;
        };
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
        let mut pixels = vec![0u8; pixel_buffer_size];
        {
            let data = slice.get_mapped_range();
            for row in 0..height_rows {
                let Some(src_start) = row.checked_mul(padded_row_bytes) else {
                    log::warn!("screenshot write skipped: source row overflow");
                    return;
                };
                let Some(dst_start) = row.checked_mul(unpadded_row_bytes) else {
                    log::warn!("screenshot write skipped: destination row overflow");
                    return;
                };
                let Some(src_end) = src_start.checked_add(unpadded_row_bytes) else {
                    log::warn!("screenshot write skipped: source row overflow");
                    return;
                };
                let Some(dst_end) = dst_start.checked_add(unpadded_row_bytes) else {
                    log::warn!("screenshot write skipped: destination row overflow");
                    return;
                };
                if src_end > data.len() || dst_end > pixels.len() {
                    log::warn!("screenshot write skipped: capture row bounds exceeded");
                    return;
                }
                pixels[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
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

/// The four thin edge rects composing a debug AABB outline, as
/// (center, size) pairs in world space.
fn debug_aabb_edges(aabb: Aabb) -> [(Vec2, Vec2); 4] {
    let thickness = DEBUG_LINE_THICKNESS;
    let size = aabb.size();
    let center = aabb.center();
    [
        (
            Vec2::new(center.x, aabb.min.y + thickness * 0.5),
            Vec2::new(size.x, thickness),
        ),
        (
            Vec2::new(center.x, aabb.max.y - thickness * 0.5),
            Vec2::new(size.x, thickness),
        ),
        (
            Vec2::new(aabb.min.x + thickness * 0.5, center.y),
            Vec2::new(thickness, size.y),
        ),
        (
            Vec2::new(aabb.max.x - thickness * 0.5, center.y),
            Vec2::new(thickness, size.y),
        ),
    ]
}

/// The thin rect representing a debug ray as (center, size, rotation), or
/// `None` for a zero-length direction.
fn debug_ray_rect(origin: Vec2, direction: Vec2) -> Option<(Vec2, Vec2, f32)> {
    let length = direction.length();
    if length <= f32::EPSILON {
        return None;
    }
    Some((
        origin + direction * 0.5,
        Vec2::new(length, DEBUG_LINE_THICKNESS),
        direction.y.atan2(direction.x),
    ))
}

/// Stages queued debug sprites into the shared sprite mesh buffers, grouped
/// by texture like the main queue, then clears the queue. Pure CPU work on
/// reused staging vectors so the queue lifecycle is testable without a GPU
/// surface.
fn stage_debug_queue(
    queue: &mut Vec<QueuedSprite>,
    vertices: &mut Vec<SpriteVertex>,
    indices: &mut Vec<u32>,
    ranges: &mut Vec<DrawRange>,
) -> usize {
    if queue.is_empty() {
        return 0;
    }
    let mut staged = 0usize;
    let mut i = 0usize;
    while i < queue.len() {
        let texture = queue[i].texture;
        let index_start = indices.len() as u32;
        let mut local = 0u32;
        while i < queue.len() && queue[i].texture == texture {
            push_sprite_mesh(&queue[i].sprite, vertices, indices);
            local += 1;
            i += 1;
        }
        let index_count = indices.len() as u32 - index_start;
        if index_count > 0 {
            ranges.push(DrawRange {
                texture,
                index_start,
                index_count,
            });
            staged += local as usize;
        }
    }
    queue.clear();
    staged
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
    use super::{
        admit_bounded, debug_aabb_edges, debug_ray_rect, logical_viewport, normalized_scale_factor,
        push_sprite_mesh, sprite_in_view, stage_debug_queue, take_with_capacity, Aabb,
        QueuedSprite, RenderBudget, RenderQuality, TextureHandle,
    };
    use crate::sprite::{Sprite, SpriteVertex};
    use glam::Vec2;
    use winit::dpi::PhysicalSize;

    #[test]
    fn render_budget_normalizes_zero_limits_and_bounds_admissions() {
        let budget = RenderBudget {
            max_sprites: 0,
            max_debug_sprites: 0,
            max_lights: 0,
        }
        .normalized();
        assert_eq!(budget.max_sprites, 1);
        assert_eq!(budget.max_debug_sprites, 1);
        assert_eq!(budget.max_lights, 1);

        let mut accepted = Vec::new();
        let mut dropped = 0;
        for value in 0..4 {
            admit_bounded(&mut accepted, value, 2, &mut dropped);
        }
        assert_eq!(accepted, vec![0, 1]);
        assert_eq!(dropped, 2);
    }

    #[test]
    fn quality_presets_use_bounded_light_budgets() {
        assert!(RenderQuality::Performance.light_budget() < RenderQuality::Balanced.light_budget());
        assert!(RenderQuality::Balanced.light_budget() < RenderQuality::Cinematic.light_budget());
    }

    #[test]
    fn returned_queue_keeps_capacity_after_staging() {
        let mut queue = Vec::with_capacity(128);
        queue.extend((0..32).map(|_| 1_u32));
        let capacity = queue.capacity();
        let mut work = take_with_capacity(&mut queue);
        work.clear();
        queue = work;
        assert_eq!(queue.capacity(), capacity);
    }

    #[test]
    fn staging_counters_match_sprite_geometry() {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let sprite = Sprite::new(Vec2::ZERO, Vec2::ONE);
        push_sprite_mesh(&sprite, &mut vertices, &mut indices);
        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
        let upload_bytes = vertices
            .len()
            .checked_mul(std::mem::size_of::<SpriteVertex>())
            .and_then(|bytes| {
                indices
                    .len()
                    .checked_mul(std::mem::size_of::<u32>())
                    .and_then(|index_bytes| bytes.checked_add(index_bytes))
            })
            .unwrap();
        assert!(upload_bytes > 0);
    }

    #[test]
    fn sprite_in_view_keeps_visible_and_edge_touching_sprites() {
        let camera_center = Vec2::ZERO;
        let view = Vec2::new(200.0, 100.0);

        // Comfortably inside.
        assert!(sprite_in_view(
            Vec2::new(50.0, -20.0),
            5.0,
            camera_center,
            view,
            0.0
        ));
        // Exactly on the boundary: distance to the rect equals the radius.
        assert!(sprite_in_view(
            Vec2::new(105.0, 0.0),
            5.0,
            camera_center,
            view,
            0.0
        ));
        // Entirely outside on the right.
        assert!(!sprite_in_view(
            Vec2::new(200.0, 0.0),
            5.0,
            camera_center,
            view,
            0.0
        ));
        // Entirely outside diagonally.
        assert!(!sprite_in_view(
            Vec2::new(150.0, 80.0),
            5.0,
            camera_center,
            view,
            0.0
        ));
        // A small margin rescues a sprite just past the edge.
        assert!(!sprite_in_view(
            Vec2::new(104.0, 0.0),
            1.0,
            camera_center,
            view,
            0.0
        ));
        assert!(sprite_in_view(
            Vec2::new(104.0, 0.0),
            1.0,
            camera_center,
            view,
            4.0
        ));
    }

    #[test]
    fn sprite_culling_uses_the_half_diagonal_for_rotated_sprites() {
        let view = Vec2::new(200.0, 100.0);
        // Half-diagonal of a 100x50 sprite; larger than either half-extent.
        let half_diagonal = Vec2::new(50.0, 25.0).length();

        // The center is inside, but rotation can swing a corner past the
        // edge, so the conservative half-diagonal radius must keep it alive.
        assert!(sprite_in_view(
            Vec2::new(95.0, 0.0),
            half_diagonal,
            Vec2::ZERO,
            view,
            0.0
        ));
        // Fully outside even accounting for rotation.
        assert!(!sprite_in_view(
            Vec2::new(160.0, 0.0),
            half_diagonal,
            Vec2::ZERO,
            view,
            0.0
        ));
    }

    #[test]
    fn debug_queue_lifecycle_is_pure_and_clears_after_staging() {
        let mut queue: Vec<QueuedSprite> = Vec::new();
        let mut vertices: Vec<SpriteVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut ranges = Vec::new();

        // An empty queue stages nothing and clears nothing.
        assert_eq!(
            stage_debug_queue(&mut queue, &mut vertices, &mut indices, &mut ranges),
            0
        );
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
        assert!(ranges.is_empty());

        // Two shapes on different textures stage as separate draw ranges.
        queue.push(QueuedSprite {
            texture: TextureHandle(0),
            sprite: Sprite::new(Vec2::ONE, Vec2::splat(4.0)),
        });
        queue.push(QueuedSprite {
            texture: TextureHandle(1),
            sprite: Sprite::new(Vec2::ZERO, Vec2::splat(4.0)),
        });
        let staged = stage_debug_queue(&mut queue, &mut vertices, &mut indices, &mut ranges);
        assert_eq!(staged, 2);
        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
        assert_eq!(ranges.len(), 2);
        assert!(queue.is_empty(), "queue clears after staging");

        // Staging an emptied queue is a no-op and leaves geometry untouched.
        assert_eq!(
            stage_debug_queue(&mut queue, &mut vertices, &mut indices, &mut ranges),
            0
        );
        assert_eq!(vertices.len(), 8);
        assert_eq!(indices.len(), 12);
    }

    #[test]
    fn debug_aabb_edges_trace_all_four_sides() {
        let aabb = Aabb::from_center_size(Vec2::new(100.0, 50.0), Vec2::new(40.0, 20.0));
        let edges = debug_aabb_edges(aabb);
        assert_eq!(edges.len(), 4);

        let thickness = super::DEBUG_LINE_THICKNESS;
        let (bottom, bottom_size) = edges[0];
        assert_eq!(bottom, Vec2::new(100.0, 40.0 + thickness * 0.5));
        assert_eq!(bottom_size, Vec2::new(40.0, thickness));
        let (top, top_size) = edges[1];
        assert_eq!(top, Vec2::new(100.0, 60.0 - thickness * 0.5));
        assert_eq!(top_size, Vec2::new(40.0, thickness));
        let (left, left_size) = edges[2];
        assert_eq!(left, Vec2::new(80.0 + thickness * 0.5, 50.0));
        assert_eq!(left_size, Vec2::new(thickness, 20.0));
        let (right, right_size) = edges[3];
        assert_eq!(right, Vec2::new(120.0 - thickness * 0.5, 50.0));
        assert_eq!(right_size, Vec2::new(thickness, 20.0));
    }

    #[test]
    fn debug_ray_rect_runs_along_its_direction_and_skips_zero_length() {
        let (position, size, rotation) =
            debug_ray_rect(Vec2::new(10.0, 20.0), Vec2::new(30.0, -40.0)).expect("nonzero ray");
        let length = 50.0_f32; // |(30, -40)|
        assert_eq!(position, Vec2::new(25.0, 0.0));
        assert_eq!(size, Vec2::new(length, super::DEBUG_LINE_THICKNESS));
        assert!((rotation - (-40.0_f32).atan2(30.0)).abs() < 1e-6);

        assert!(debug_ray_rect(Vec2::ZERO, Vec2::ZERO).is_none());
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
