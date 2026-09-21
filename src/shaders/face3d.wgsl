// Face3D shader — flat-shaded triangle fill for DXF 3DFACE entities.
//
// Vertex layout (28 bytes):
//   position  [f32; 3]   offset  0   12 B
//   color     [f32; 4]   offset 12   16 B

struct Uniforms {
    viewport_size:       vec2<f32>,
    world_per_pixel:     f32,
    lwdisplay_enable:    f32,
    flat_shade:          f32,
    transparency_enable: f32,
    _pad:                vec2<f32>,
    // Relative-to-eye (double-single): see wire.wgsl.
    view_rot:            mat4x4<f32>,
    eye_high:            vec3<f32>,
    _pad_eh:             f32,
    eye_low:             vec3<f32>,
    _pad_el:             f32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position:     vec3<f32>,
    @location(1) color:        vec4<f32>,
    @location(2) draw_depth:   f32,
    @location(3) position_low: vec3<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       color:    vec4<f32>,
};

// Draw-order depth bias (see draw_order.wgsl). Signed draw_depth; 0.0 (all 3D
// surface faces — 3DFACE, PolyfaceMesh, PolygonMesh) leaves real depth
// untouched so they occlude against solids; only 2D fills order by rank.

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let rel = (v.position - u.eye_high) + (v.position_low - u.eye_low);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.clip_pos = apply_draw_order(out.clip_pos, v.draw_depth);
    out.color    = v.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return in.color;
}
