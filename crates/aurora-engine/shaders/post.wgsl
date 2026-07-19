struct PostUniforms {
    time: f32,
    bloom_threshold: f32,
    bloom_intensity: f32,
    vignette_strength: f32,
    chromatic: f32,
    enabled: f32,
    texel_x: f32,
    texel_y: f32,
}

@group(0) @binding(0)
var t_scene: texture_2d<f32>;
@group(0) @binding(1)
var s_scene: sampler;
@group(0) @binding(2)
var<uniform> u: PostUniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Fullscreen triangle
    var out: VsOut;
    let x = f32(i32(idx & 1u) * 4 - 1);
    let y = f32(i32(idx >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>(x, y) * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
    return out;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn tonemap_aces(c: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let cc = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((c * (a * c + b)) / (c * (cc * c + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let texel = vec2<f32>(u.texel_x, u.texel_y);

    if (u.enabled < 0.5) {
        return textureSample(t_scene, s_scene, uv);
    }

    // Chromatic aberration
    let ca = u.chromatic;
    let dir = (uv - vec2<f32>(0.5, 0.5));
    let r = textureSample(t_scene, s_scene, uv + dir * ca).r;
    let g = textureSample(t_scene, s_scene, uv).g;
    let b = textureSample(t_scene, s_scene, uv - dir * ca).b;
    var color = vec3<f32>(r, g, b);

    // Cheap bloom: threshold + multi-tap blur on bright areas
    var bloom = vec3<f32>(0.0);
    var wsum = 0.0;
    let offs = array<vec2<f32>, 9>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, -1.0),
    );
    let weights = array<f32, 9>(4.0, 2.0, 2.0, 2.0, 2.0, 1.0, 1.0, 1.0, 1.0);
    let blur_scale = 2.5;
    for (var i = 0; i < 9; i++) {
        let sample_uv = uv + offs[i] * texel * blur_scale;
        let s = textureSample(t_scene, s_scene, sample_uv).rgb;
        let bright = max(luminance(s) - u.bloom_threshold, 0.0);
        let w = weights[i];
        bloom += s * bright * w;
        wsum += w;
    }
    bloom = bloom / max(wsum, 0.001);
    color = color + bloom * u.bloom_intensity;

    // Vignette
    let d = length(uv - vec2<f32>(0.5, 0.5));
    let vig = smoothstep(0.85, 0.25, d * (0.85 + u.vignette_strength));
    color *= mix(1.0, vig, u.vignette_strength);

    // Subtle pulse
    color *= 0.97 + 0.03 * sin(u.time * 1.5);

    return vec4<f32>(tonemap_aces(color), 1.0);
}
