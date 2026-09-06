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

struct VertIn {
    @location(0) pos: vec3<f32>,
    @location(1) pos_low: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) source_depth: f32,
    @location(5) translation: vec3<f32>,
    @location(6) translation_low: vec3<f32>,
    @location(7) instance_depth: f32,
}

struct VertOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

const DRAW_ORDER_BIAS: f32 = 0.001;

@vertex
fn vs_main(v: VertIn) -> VertOut {
    var out: VertOut;
    let rel = (v.pos + v.translation - u.eye_high)
        + (v.pos_low + v.translation_low - u.eye_low);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.clip_pos.z = out.clip_pos.z
        - (v.source_depth + v.instance_depth) * DRAW_ORDER_BIAS * out.clip_pos.w;
    out.uv = v.uv;
    out.color = v.color;
    return out;
}

@fragment
fn fs_main(in: VertOut) -> @location(0) vec4<f32> {
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);

    let dims_u = textureDimensions(atlas_tex);
    let dims = vec2<f32>(f32(dims_u.x), f32(dims_u.y));

    let texel_dx = uv_dx * dims;
    let texel_dy = uv_dy * dims;

    let footprint = max(length(texel_dx), length(texel_dy));

    let sd_center = textureSampleLevel(
        atlas_tex,
        atlas_samp,
        in.uv,
        0.0
    ).r;

    let aa = max(fwidth(sd_center), 1e-4);

    var a: f32;

    if (footprint <= 1.0) {
        // Large text / approximately 1:1 sampling.
        a = smoothstep(
            0.5 - aa,
            0.5 + aa,
            sd_center
        );
    } else if (footprint <= 4.0) {
        // Moderate minification: 2x2 sampling.
        let s0 = textureSampleLevel(
            atlas_tex,
            atlas_samp,
            in.uv - uv_dx * 0.25 - uv_dy * 0.25,
            0.0
        ).r;

        let s1 = textureSampleLevel(
            atlas_tex,
            atlas_samp,
            in.uv + uv_dx * 0.25 - uv_dy * 0.25,
            0.0
        ).r;

        let s2 = textureSampleLevel(
            atlas_tex,
            atlas_samp,
            in.uv - uv_dx * 0.25 + uv_dy * 0.25,
            0.0
        ).r;

        let s3 = textureSampleLevel(
            atlas_tex,
            atlas_samp,
            in.uv + uv_dx * 0.25 + uv_dy * 0.25,
            0.0
        ).r;

        let a0 = smoothstep(0.5 - aa, 0.5 + aa, s0);
        let a1 = smoothstep(0.5 - aa, 0.5 + aa, s1);
        let a2 = smoothstep(0.5 - aa, 0.5 + aa, s2);
        let a3 = smoothstep(0.5 - aa, 0.5 + aa, s3);

        a = (a0 + a1 + a2 + a3) * 0.25;
    } else {
        // Strong minification: 4x4 sampling over the full screen-pixel
        // footprint. At this scale the glyph covers very few fragments, so
        // the additional texture reads remain localized to small text.
        var sum = 0.0;

        for (var y: i32 = 0; y < 4; y = y + 1) {
            for (var x: i32 = 0; x < 4; x = x + 1) {
                let fx = (f32(x) + 0.5) / 4.0 - 0.5;
                let fy = (f32(y) + 0.5) / 4.0 - 0.5;

                let sd = textureSampleLevel(
                    atlas_tex,
                    atlas_samp,
                    in.uv + uv_dx * fx + uv_dy * fy,
                    0.0
                ).r;

                sum = sum + smoothstep(
                    0.5 - aa,
                    0.5 + aa,
                    sd
                );
            }
        }

        a = sum / 16.0;
    }

    if (a <= 0.0) {
        discard;
    }

    return vec4<f32>(
        in.color.rgb,
        in.color.a * a
    );
}
