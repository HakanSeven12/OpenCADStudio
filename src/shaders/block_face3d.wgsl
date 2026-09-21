struct Uniforms {
    viewport_size: vec2<f32>,
    world_per_pixel: f32,
    lwdisplay_enable: f32,
    flat_shade: f32,
    transparency_enable: f32,
    _pad: vec2<f32>,
    view_rot: mat4x4<f32>,
    eye_high: vec3<f32>,
    _pad_eh: f32,
    eye_low: vec3<f32>,
    _pad_el: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) source_depth: f32,
    @location(3) position_low: vec3<f32>,
    @location(4) translation: vec3<f32>,
    @location(5) translation_low: vec3<f32>,
    @location(6) instance_depth: f32,
}

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let rel = (v.position + v.translation - u.eye_high)
        + (v.position_low + v.translation_low - u.eye_low);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.clip_pos = apply_draw_order(out.clip_pos, v.source_depth + v.instance_depth);
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
