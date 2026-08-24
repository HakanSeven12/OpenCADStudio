struct Uniforms {
    viewport_size: vec2<f32>,
    world_per_pixel: f32,
    lwdisplay_enable: f32,
    flat_shade: f32,
    transparency_enable: f32,
    linetype_scale: f32,
    _pad: f32,
    view_rot: mat4x4<f32>,
    eye_high: vec3<f32>,
    _pad_eh: f32,
    eye_low: vec3<f32>,
    _pad_el: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

struct WireConst {
    color: vec4<f32>,
    pat0: vec4<f32>,
    pat1: vec4<f32>,
    half_width: f32,
    pattern_length: f32,
    draw_depth: f32,
    align_end: f32,
    align_total: f32,
    world_half_width: f32,
    _pad1: f32,
    _pad2: f32,
    marker_origin_high: vec4<f32>,
    marker_origin_low: vec4<f32>,
    marker_normal_scale: vec4<f32>,
}
@group(1) @binding(0) var<uniform> wire_const: WireConst;

struct VertexIn {
    @location(0) pos_a: vec3<f32>,
    @location(1) pos_b: vec3<f32>,
    @location(2) pos_a_low: vec3<f32>,
    @location(3) pos_b_low: vec3<f32>,
    @location(4) distances: vec2<f32>,
    @location(5) taper_ratio: vec2<f32>,
    @location(6) translation: vec3<f32>,
    @location(7) translation_low: vec3<f32>,
    @location(8) depth: vec2<f32>,
}

const DRAW_ORDER_BIAS: f32 = 0.001;

struct VertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) distance: f32,
    @location(2) pattern_length: f32,
    @location(3) pat0: vec4<f32>,
    @location(4) pat1: vec4<f32>,
    @location(5) @interpolate(flat) min_elem: f32,
    @location(6) @interpolate(flat) align_end: f32,
    @location(7) @interpolate(flat) align_total: f32,
    @location(8) cap: vec2<f32>,
    @location(9) @interpolate(flat) cap_ends: vec3<f32>,
}

fn resolve_hw(taper_ratio: f32, world_hw: f32, px_hw: f32) -> f32 {
    if taper_ratio > 0.0 {
        return max((taper_ratio * world_hw) / u.world_per_pixel, 0.5);
    }
    if world_hw > 0.0 {
        return max(world_hw / u.world_per_pixel, 0.5);
    }
    return select(0.5, px_hw, u.lwdisplay_enable > 0.5);
}

fn marker_relative(
    position_high: vec3<f32>,
    position_low: vec3<f32>,
    translation: vec3<f32>,
    translation_low: vec3<f32>,
) -> vec3<f32> {
    if wire_const.marker_normal_scale.w <= 0.0 {
        return (position_high + translation - u.eye_high)
            + (position_low + translation_low - u.eye_low);
    }
    let origin_high = wire_const.marker_origin_high.xyz;
    let origin_low = wire_const.marker_origin_low.xyz;
    let origin_relative = (origin_high + translation - u.eye_high)
        + (origin_low + translation_low - u.eye_low);
    let delta = (position_high - origin_high) + (position_low - origin_low);
    let normal = normalize(wire_const.marker_normal_scale.xyz);
    let axial = normal * dot(delta, normal);
    let planar = delta - axial;
    let origin_clip = u.view_rot * vec4<f32>(origin_relative, 1.0);
    let projection_scale = max(length(vec3<f32>(
        u.view_rot[0].y,
        u.view_rot[1].y,
        u.view_rot[2].y,
    )), 1e-12);
    let view_height = 2.0 * max(abs(origin_clip.w), 1e-6) / projection_scale;
    let world_size = wire_const.marker_normal_scale.w * 0.01 * view_height;
    return origin_relative + axial + planar * world_size;
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, in: VertexIn) -> VertexOut {
    let corner = vertex_index % 6u;
    let which_end_arr = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let side_arr = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let which_end = which_end_arr[corner];
    let side = side_arr[corner];

    let rel_a = marker_relative(in.pos_a, in.pos_a_low, in.translation, in.translation_low);
    let rel_b = marker_relative(in.pos_b, in.pos_b_low, in.translation, in.translation_low);
    let clip_a = u.view_rot * vec4<f32>(rel_a, 1.0);
    let clip_b = u.view_rot * vec4<f32>(rel_b, 1.0);
    let screen_a = clip_a.xy / clip_a.w * u.viewport_size * 0.5;
    let screen_b = clip_b.xy / clip_b.w * u.viewport_size * 0.5;
    let segment = screen_b - screen_a;
    let segment_length = length(segment);
    var direction = vec2<f32>(1.0, 0.0);
    if segment_length > 1e-4 {
        direction = segment / segment_length;
    }
    let perpendicular = vec2<f32>(-direction.y, direction.x);
    let clip_position = mix(clip_a, clip_b, which_end);
    let half_width_a = resolve_hw(
        in.taper_ratio.x,
        wire_const.world_half_width,
        wire_const.half_width,
    );
    let half_width_b = resolve_hw(
        in.taper_ratio.y,
        wire_const.world_half_width,
        wire_const.half_width,
    );
    let half_width = mix(half_width_a, half_width_b, which_end);
    let extension = which_end * 2.0 - 1.0;
    let offset_px = perpendicular * half_width * side
        + direction * half_width * extension;
    let ndc_offset = offset_px / (u.viewport_size * 0.5);
    let final_clip = clip_position
        + vec4<f32>(ndc_offset * clip_position.w, 0.0, 0.0);

    let scale = u.linetype_scale;
    var min_element = wire_const.pattern_length * scale;
    let elements = array<f32, 8>(
        wire_const.pat0.x * scale,
        wire_const.pat0.y * scale,
        wire_const.pat0.z * scale,
        wire_const.pat0.w * scale,
        wire_const.pat1.x * scale,
        wire_const.pat1.y * scale,
        wire_const.pat1.z * scale,
        wire_const.pat1.w * scale,
    );
    for (var i = 0u; i < 8u; i++) {
        let value = abs(elements[i]);
        if value > 0.0 && value < min_element {
            min_element = value;
        }
    }

    var out: VertexOut;
    out.clip_pos = final_clip;
    out.clip_pos.z -= in.depth.x * DRAW_ORDER_BIAS * out.clip_pos.w;
    out.color = wire_const.color;
    out.distance = mix(in.distances.x, in.distances.y, which_end)
        + extension * half_width * u.world_per_pixel;
    out.pattern_length = wire_const.pattern_length * scale;
    out.pat0 = wire_const.pat0 * scale;
    out.pat1 = wire_const.pat1 * scale;
    out.min_elem = min_element;
    out.align_end = wire_const.align_end * scale;
    out.align_total = wire_const.align_total;
    out.cap = vec2<f32>(
        which_end * segment_length + extension * half_width,
        half_width * side,
    );
    out.cap_ends = vec3<f32>(segment_length, half_width_a, half_width_b);
    return out;
}

fn in_dash(
    distance: f32,
    pattern_length: f32,
    pat0: vec4<f32>,
    pat1: vec4<f32>,
    align_end: f32,
    align_total: f32,
) -> bool {
    let elements = array<f32, 8>(
        pat0.x, pat0.y, pat0.z, pat0.w,
        pat1.x, pat1.y, pat1.z, pat1.w,
    );
    var count = 0u;
    for (var i = 0u; i < 8u; i++) {
        if elements[i] != 0.0 {
            count = i + 1u;
        }
    }
    var value: f32;
    if align_total > 0.0 {
        if distance <= align_end || distance >= align_total - align_end {
            return true;
        }
        var first_dash = 0.0;
        for (var i = 0u; i < count; i++) {
            if elements[i] > 0.0 {
                first_dash = elements[i];
                break;
            }
        }
        value = ((distance - align_end + first_dash) % pattern_length
            + pattern_length) % pattern_length;
    } else {
        value = ((distance % pattern_length) + pattern_length) % pattern_length;
    }
    var position = 0.0;
    let dot_half = u.world_per_pixel * 0.75;
    for (var i = 0u; i < count; i++) {
        let element = elements[i];
        if element == 0.0 {
            let delta = abs(value - position);
            if min(delta, pattern_length - delta) <= dot_half {
                return true;
            }
        } else if element > 0.0 {
            if value >= position && value < position + element {
                return true;
            }
            position += element;
        } else {
            position -= element;
        }
    }
    return false;
}

fn clipped_cap(cap: vec2<f32>, ends: vec3<f32>) -> bool {
    if cap.x < 0.0 {
        return length(cap) > ends.y;
    }
    if cap.x > ends.x {
        return length(vec2<f32>(cap.x - ends.x, cap.y)) > ends.z;
    }
    return false;
}

fn visible_fragment(in: VertexOut) -> bool {
    if clipped_cap(in.cap, in.cap_ends) {
        return false;
    }
    if in.pattern_length > 0.0 && in.min_elem >= u.world_per_pixel {
        return in_dash(
            in.distance,
            in.pattern_length,
            in.pat0,
            in.pat1,
            in.align_end,
            in.align_total,
        );
    }
    return true;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if !visible_fragment(in) {
        discard;
    }
    let alpha = select(1.0, in.color.a, u.transparency_enable > 0.5);
    return vec4<f32>(in.color.rgb, alpha);
}

@fragment
fn fs_black(in: VertexOut) -> @location(0) vec4<f32> {
    if !visible_fragment(in) {
        discard;
    }
    let alpha = select(1.0, in.color.a, u.transparency_enable > 0.5);
    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}
