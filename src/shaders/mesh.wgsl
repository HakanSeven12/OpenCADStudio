// Mesh shader — renders triangle meshes (truck Shell/Solid tessellation).
//
// Vertex layout: position [f32;3], normal [f32;3], color [f32;4]  (40 bytes)
//
// Lighting: simple half-Lambert with a fixed directional light. Two
// shading paths share this shader, picked per-frame via `u.flat_shade`:
//   - 0.0 → per-vertex normals interpolated to the fragment (Gouraud).
//   - 1.0 → per-triangle face normal from screen-space derivatives
//     `cross(dpdx(pos), dpdy(pos))`, so each triangle reads as a single
//     flat shade (FlatShaded).

struct Uniforms {
    view_proj:        mat4x4<f32>,
    camera_pos:       vec4<f32>,
    viewport_size:    vec2<f32>,
    world_per_pixel:  f32,
    lwdisplay_enable: f32,
    flat_shade:       f32,
    _pad0:            f32,
    _pad1:            f32,
    _pad2:            f32,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal:   vec3<f32>,
    @location(2) color:    vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       color:     vec4<f32>,
    @location(1)       normal:    vec3<f32>,
    @location(2)       world_pos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_pos  = u.view_proj * vec4<f32>(v.position, 1.0);
    out.color     = v.color;
    out.normal    = v.normal;
    out.world_pos = v.position;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Fixed directional light (world space, tilted toward the viewer).
    let light_dir = normalize(vec3<f32>(0.4, 0.8, 0.6));
    var n: vec3<f32>;
    if (u.flat_shade > 0.5) {
        // Per-triangle face normal: derivatives of the interpolated
        // world position are constant across the primitive, so every
        // fragment in the same triangle sees the same normal.
        n = normalize(cross(dpdx(in.world_pos), dpdy(in.world_pos)));
    } else {
        n = normalize(in.normal);
    }
    // Half-Lambert: [0.5, 1.0] so the dark side isn't fully black.
    let diff = clamp(dot(n, light_dir), 0.0, 1.0) * 0.5 + 0.5;
    let rgb  = in.color.rgb * diff;
    return vec4<f32>(rgb, in.color.a);
}
