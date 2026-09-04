struct PostUniforms {
    time: f32,
    bloom_threshold: f32,
    bloom_intensity: f32,
    vignette_strength: f32,
    chromatic: f32,
    enabled: f32,
    texel_x: f32,
    texel_y: f32,
    light_count: f32,
    film_grain: f32,
    speed_streaks: f32,
    _pad0: f32,
    lights: array<vec4<f32>, 16>,
    light_colors: array<vec4<f32>, 16>,
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

fn hash_noise(seed: vec2<f32>) -> f32 {
    return fract(sin(dot(seed, vec2<f32>(12.9898, 78.233))) * 43758.5453);
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

    // Analytic point lights are added in HDR space. Radius is expressed in
    // viewport-height UV units so circles remain circular on wide displays.
    let aspect = u.texel_y / u.texel_x;
    for (var i = 0; i < 16; i++) {
        if (f32(i) >= u.light_count) {
            break;
        }
        let light = u.lights[i];
        var delta = uv - light.xy;
        delta.x *= aspect;
        let distance = length(delta) / light.z;
        let falloff = max(1.0 - distance, 0.0);
        color += u.light_colors[i].rgb * falloff * falloff * light.w;
    }

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

    var tonemapped = tonemap_aces(color);

    // Animated film grain, applied post-tonemap like a camera sensor overlay.
    if (u.film_grain > 0.001) {
        let grain_uv = uv * vec2<f32>(u.texel_y / u.texel_x, 1.0) * 540.0;
        let noise = hash_noise(grain_uv + vec2<f32>(fract(u.time * 13.7) * 91.0, 0.0)) - 0.5;
        tonemapped = clamp(tonemapped + vec3<f32>(noise) * u.film_grain * 0.14, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // Radial dash streaks, added post-tonemap like the grain overlay. The
    // view angle is encoded through the unit direction so the three noise
    // octaves rotate seamlessly (no atan2 branch-cut seam at 180 degrees).
    if (u.speed_streaks > 0.001) {
        let delta = uv - vec2<f32>(0.5, 0.5);
        let radius = max(length(delta), 1e-4);
        let dir = delta / radius;
        var streak = hash_noise(dir * 14.0 + vec2<f32>(u.time * 5.0, 0.0));
        streak += hash_noise(dir * 29.0 - vec2<f32>(u.time * 8.0, 0.0)) * 0.5;
        streak += hash_noise(dir * 53.0 + vec2<f32>(u.time * 12.0, 0.0)) * 0.25;
        streak = streak / 1.75;
        let fade = 1.0 - smoothstep(0.05, 0.85, radius);
        let add = streak * fade * u.speed_streaks * 0.25;
        tonemapped = clamp(tonemapped + vec3<f32>(add), vec3<f32>(0.0), vec3<f32>(1.0));
    }

    return vec4<f32>(tonemapped, 1.0);
}
