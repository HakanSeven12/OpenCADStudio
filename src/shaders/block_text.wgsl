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
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_samp: sampler;

// Atlas texels per glyph unit and the field's half range in glyph units
// (scene::text::sdf_atlas PX_PER_UNIT / SPREAD_UNITS).
const PX_PER_UNIT: f32 = 6.0;
const SPREAD_UNITS: f32 = 1.5;

struct VertIn {
    @location(0) pos: vec3<f32>,
    @location(1) pos_low: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) source_depth: f32,
    @location(5) translation: vec3<f32>,
    @location(6) translation_low: vec3<f32>,
    @location(7) instance_depth: f32,
    @location(8) pen: f32,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) pen: f32,
}

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var out: VertOut;
    let rel = (v.pos + v.translation - u.eye_high)
        + (v.pos_low + v.translation_low - u.eye_low);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.clip_pos = apply_draw_order(out.clip_pos, v.source_depth + v.instance_depth);
    out.uv = v.uv;
    out.color = v.color;
    out.pen = v.pen;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    let dims = vec2<f32>(textureDimensions(atlas_tex));
    let footprint = max(length(uv_dx * dims), length(uv_dy * dims));
    let sd_center = textureSampleLevel(atlas_tex, atlas_samp, in.uv, 0.0).r;
    let aa = max(fwidth(sd_center), 1e-4);
    var alpha = smoothstep(0.5 - aa, 0.5 + aa, sd_center);

    if footprint > 1.0 {
        // Average coverage over the pixel footprint so thin strokes survive minification.
        let samples = select(4, 2, footprint <= 4.0);
        var sum = 0.0;
        for (var y = 0; y < samples; y += 1) {
            for (var x = 0; x < samples; x += 1) {
                let offset = (vec2<f32>(f32(x), f32(y)) + 0.5) / f32(samples) - 0.5;
                let sd = textureSampleLevel(
                    atlas_tex, atlas_samp, in.uv + uv_dx * offset.x + uv_dy * offset.y, 0.0
                ).r;
                sum += smoothstep(0.5 - aa, 0.5 + aa, sd);
            }
        }
        alpha = sum / f32(samples * samples);
    }
    if in.pen > 0.0 {
        // Stroke fonts have no width of their own: once the baked pen band
        // is wider than a pixel, draw the centreline one pixel wide (the
        // field holds pen - distance, SPREAD_UNITS per half range).
        let units_per_px = max(footprint, 1e-4) / PX_PER_UNIT;
        if in.pen > 0.5 * units_per_px {
            let from_centre = in.pen - (sd_center - 0.5) * 2.0 * SPREAD_UNITS;
            alpha = clamp(1.0 - abs(from_centre) / units_per_px, 0.0, 1.0);
        }
    }
    if alpha <= 0.0 {
        discard;
    }
    return vec4<f32>(in.color.rgb, select(1.0, in.color.a, u.transparency_enable > 0.5) * alpha);
}
