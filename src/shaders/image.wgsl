// Textured-quad shader for raster images (RasterImage entity).
// Renders a four-vertex quad (two triangles) with a sampled texture.

// ── Bind group 0: shared projection uniforms ─────────────────────────────────
// Must match the shared `Uniforms` struct (scene::pipeline::uniforms, 112 B).
struct Uniforms {
    viewport_size:      vec2<f32>,
    world_per_pixel:    f32,
    lwdisplay_enable:   f32,
    flat_shade:         f32,
    transparency_enable: f32,
    _pad:               vec2<f32>,
    // Relative-to-eye (double-single): see wire.wgsl.
    view_rot:           mat4x4<f32>,
    eye_high:           vec3<f32>,
    _pad_eh:            f32,
    eye_low:            vec3<f32>,
    _pad_el:            f32,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Bind group 1: per-image texture + sampler ────────────────────────────────
@group(1) @binding(0) var img_texture: texture_2d<f32>;
@group(1) @binding(1) var img_sampler: sampler;

// Per-image params (fade, clip flag, etc.)
struct ImageParams {
    opacity:    f32,
    draw_depth: f32,   // signed (-1,1) draw-order bias; 0 = neutral
    _pad1:      f32,
    _pad2:      f32,
};
@group(1) @binding(2) var<uniform> img_params: ImageParams;

// ── Vertex stage ──────────────────────────────────────────────────────────────
struct VertIn {
    @location(0) pos:     vec3<f32>,
    @location(1) uv:      vec2<f32>,
    @location(2) pos_low: vec3<f32>,
    @location(3) translation: vec3<f32>,
    @location(4) translation_low: vec3<f32>,
    @location(5) draw_depth: f32,
};

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)       uv:       vec2<f32>,
};

@vertex
fn vs_main(in: VertIn) -> VertOut {
    var out: VertOut;
    let rel = (in.pos + in.translation - u.eye_high)
        + (in.pos_low + in.translation_low - u.eye_low);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.clip_pos = apply_draw_order(out.clip_pos, img_params.draw_depth + in.draw_depth);
    out.uv = in.uv;
    return out;
}

// ── Fragment stage ────────────────────────────────────────────────────────────
@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let col = textureSample(img_texture, img_sampler, in.uv);
    return vec4<f32>(col.rgb, col.a * img_params.opacity);
}
