// Depth-only pass for the directional-light shadow map. Reuses the per-object
// uniform buffers (group 1) shared with the main mesh3d pipeline.

struct LightSpace {
    light_view_proj: mat4x4<f32>,
}

@group(0) @binding(0)
var<uniform> light_space: LightSpace;

struct ObjectModel {
    model: mat4x4<f32>,
}

@group(1) @binding(0)
var<uniform> object: ObjectModel;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> @builtin(position) vec4<f32> {
    let world = object.model * vec4<f32>(in.position, 1.0);
    return light_space.light_view_proj * world;
}
