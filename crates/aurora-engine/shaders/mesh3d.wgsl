struct Scene {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    // rgb = color, a = intensity
    light_color: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> scene: Scene;

struct Object {
    model: mat4x4<f32>,
    base_color: vec4<f32>,
    emissive: vec4<f32>,
    // x = metallic, y = roughness
    metallic_roughness: vec4<f32>,
}

@group(1) @binding(0)
var<uniform> object: Object;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world = object.model * vec4<f32>(in.position, 1.0);
    out.clip_position = scene.view_proj * world;
    out.world_pos = world.xyz;
    // Assumes a uniformly scaled model matrix, so the 3x3 part alone is
    // enough to rotate normals correctly without an inverse-transpose.
    out.world_normal = normalize((object.model * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    return out;
}

const PI: f32 = 3.14159265359;

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denom = (n_dot_h * n_dot_h) * (a2 - 1.0) + 1.0;
    return a2 / max(PI * denom * denom, 1e-6);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let ggx_v = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let ggx_l = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return ggx_v * ggx_l;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let v = normalize(scene.camera_pos.xyz - in.world_pos);
    let l = normalize(-scene.light_dir.xyz);
    let h = normalize(v + l);

    let metallic = object.metallic_roughness.x;
    let roughness = clamp(object.metallic_roughness.y, 0.04, 1.0);
    let base_color = object.base_color.rgb;

    let f0 = mix(vec3<f32>(0.04), base_color, metallic);

    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_h = max(dot(n, h), 0.0);
    let h_dot_v = max(dot(h, v), 0.0);

    let d = distribution_ggx(n_dot_h, roughness);
    let g = geometry_smith(n_dot_v, n_dot_l, roughness);
    let f = fresnel_schlick(h_dot_v, f0);

    let specular = (d * g * f) / max(4.0 * n_dot_v * n_dot_l, 1e-4);

    let k_s = f;
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let diffuse = k_d * base_color / PI;

    let radiance = scene.light_color.rgb * scene.light_color.a;
    var color = (diffuse + specular) * radiance * n_dot_l;

    // Small flat ambient term so unlit faces read as dim, not pure black.
    color += base_color * 0.03;
    color += object.emissive.rgb;

    return vec4<f32>(color, object.base_color.a);
}
