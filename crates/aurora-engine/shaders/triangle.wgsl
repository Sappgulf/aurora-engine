struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
}

struct Uniforms {
    time: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let angle = uniforms.time * 1.2;
    let c = cos(angle);
    let s = sin(angle);
    let rotated = vec2<f32>(
        in.position.x * c - in.position.y * s,
        in.position.x * s + in.position.y * c,
    );
    // Gentle pulse
    let scale = 0.85 + 0.08 * sin(uniforms.time * 2.4);
    out.clip_position = vec4<f32>(rotated * scale, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft glow boost on the primaries
    let boosted = in.color * (0.85 + 0.15 * sin(uniforms.time * 3.0 + in.color.r * 6.28));
    return vec4<f32>(boosted, 1.0);
}
