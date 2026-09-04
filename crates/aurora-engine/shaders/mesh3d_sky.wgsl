// Fullscreen gradient sky drawn at the start of the Mesh3D pass, before any
// geometry. Reconstructs the view ray through the inverse view-projection and
// blends the configured horizon/zenith colors, plus a soft sun disk toward
// the directional light.

struct Scene {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    light_dir: vec4<f32>,
    // rgb = color, a = intensity
    light_color: vec4<f32>,
    // rgb = zenith color, a = sun intensity
    sky_zenith: vec4<f32>,
    // rgb = horizon color
    sky_horizon: vec4<f32>,
    shadow_params: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> scene: Scene;

struct SkyOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> SkyOutput {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    let p = corners[vertex_index];
    var out: SkyOutput;
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.ndc = p;
    return out;
}

@fragment
fn fs_main(in: SkyOutput) -> @location(0) vec4<f32> {
    let far = scene.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let ray = normalize(far.xyz / far.w - scene.camera_pos.xyz);

    let up = clamp(ray.y, -1.0, 1.0);
    let t = pow(clamp(up, 0.0, 1.0), 0.65);
    var color = mix(scene.sky_horizon.rgb, scene.sky_zenith.rgb, t);
    color *= mix(0.35, 1.0, smoothstep(-0.35, 0.05, up));

    let sun_dir = normalize(-scene.light_dir.xyz);
    let sun_dot = clamp(dot(ray, sun_dir), 0.0, 1.0);
    let disk = smoothstep(0.9992, 0.9997, sun_dot);
    let glow = pow(sun_dot, 320.0) * 0.35;
    let sun = scene.light_color.rgb * scene.light_color.a * scene.sky_zenith.a;
    color += sun * (disk * 6.0 + glow);

    return vec4<f32>(color, 1.0);
}
