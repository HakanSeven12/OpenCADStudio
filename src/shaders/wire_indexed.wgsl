// Wire shader (native) — same as wire.wgsl, but the per-wire constants
// (color / half_width / dash pattern / draw_depth) live in a per-wire storage
// buffer indexed by `wire_id` instead of being replicated on every segment
// instance. Cuts the instance from 104 B to one 64-byte cache line and removes
// the redundant per-segment re-fetch of constants. WebGL2 has no vertex-stage
// storage buffers, so the wasm build uses wire.wgsl (fat instance) instead.

struct Uniforms {
    viewport_size:    vec2<f32>,
    world_per_pixel:  f32,
    lwdisplay_enable: f32,
    flat_shade: f32,
    transparency_enable: f32,
    linetype_scale: f32,
    lineweight_scale: f32,
    view_rot:         mat4x4<f32>,
    eye_high:         vec3<f32>,
    _pad_eh:          f32,
    eye_low:          vec3<f32>,
    _pad_el:          f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// Per-wire constants (std430). Must match `WireConst` in wire_gpu.rs.
struct WireConst {
    color:          vec4<f32>,
    pat0:           vec4<f32>,
    pat1:           vec4<f32>,
    half_width:     f32,
    pattern_length: f32,
    draw_depth:     f32,
    align_end:      f32,
    align_total:    f32,
    world_half_width: f32,
    _pad1:          f32,
    _pad2:          f32,
    marker_origin_high: vec4<f32>,
    marker_origin_low: vec4<f32>,
    marker_normal_scale: vec4<f32>,
}
@group(1) @binding(0) var<storage, read> wire_consts: array<WireConst>;

struct InstanceIn {
    @location(0) pos_a:      vec3<f32>,
    @location(1) pos_b:      vec3<f32>,
    @location(2) pos_a_low:  vec3<f32>,
    @location(3) pos_b_low:  vec3<f32>,
    @location(4) distance_a: f32,
    @location(5) distance_b: f32,
    @location(6) wire_id:    u32,
    // Per-endpoint width / per-wire maximum width. Vertex UNORM16 conversion
    // expands this to 0..1; zero keeps the constant-width fallback.
    @location(7) taper_ratio: vec2<f32>,
}

const DRAW_ORDER_BIAS: f32 = 0.001;
const MODEL_LINEWEIGHT_BOOST: f32 = 2.0;
const MODEL_LINEWEIGHT_MAX_PX: f32 = 10.0;

struct VertexOut {
    @builtin(position)              clip_pos:       vec4<f32>,
    @location(0)                    color:          vec4<f32>,
    @location(1)                    distance:       f32,
    @location(2)                    pattern_length: f32,
    @location(3)                    pat0:           vec4<f32>,
    @location(4)                    pat1:           vec4<f32>,
    @location(5) @interpolate(flat) min_elem:       f32,
    @location(6) @interpolate(flat) align_end:      f32,
    @location(7) @interpolate(flat) align_total:    f32,
    // Round-cap support: (along, across) of this fragment in screen pixels,
    // where `along` runs -hw_a … seg_len+hw_b over the extended quad and
    // `across` is the signed distance from the centreline.
    @location(8)                    cap:            vec2<f32>,
    // (segment pixel length, end half-width at A, end half-width at B).
    @location(9) @interpolate(flat) cap_ends:       vec3<f32>,
}

// Half-width of one segment end: a tapered band's own end width wins, then a
// constant world-unit band, then the screen-pixel lineweight (LWDISPLAY off
// collapses to a hairline).
fn resolve_hw(taper_ratio: f32, world_hw: f32, px_hw: f32) -> f32 {
    if taper_ratio > 0.0 {
        return max((taper_ratio * world_hw) / u.world_per_pixel, 0.5);
    }
    if world_hw > 0.0 { return max(world_hw / u.world_per_pixel, 0.5); }
    var display_hw = max(px_hw * u.lineweight_scale, 0.5);
    if u.lineweight_scale < 0.0 {
        let scale = -u.lineweight_scale;
        let base_hw = select(
            min(px_hw * MODEL_LINEWEIGHT_BOOST, MODEL_LINEWEIGHT_MAX_PX * 0.5),
            0.5,
            px_hw <= 0.5,
        );
        display_hw = max(base_hw * scale, 0.5);
    }
    return select(0.5, display_hw, u.lwdisplay_enable > 0.5);
}

fn marker_relative(position_high: vec3<f32>, position_low: vec3<f32>, c: WireConst) -> vec3<f32> {
    if c.marker_normal_scale.w <= 0.0 {
        return (position_high - u.eye_high) + (position_low - u.eye_low);
    }
    let origin_high = c.marker_origin_high.xyz;
    let origin_low = c.marker_origin_low.xyz;
    let origin_relative = (origin_high - u.eye_high) + (origin_low - u.eye_low);
    let delta = (position_high - origin_high) + (position_low - origin_low);
    let normal = normalize(c.marker_normal_scale.xyz);
    let axial = normal * dot(delta, normal);
    let planar = delta - axial;
    let origin_clip = u.view_rot * vec4<f32>(origin_relative, 1.0);
    let projection_scale = max(length(vec3<f32>(
        u.view_rot[0].y,
        u.view_rot[1].y,
        u.view_rot[2].y,
    )), 1e-12);
    let view_height = 2.0 * max(abs(origin_clip.w), 1e-6) / projection_scale;
    let world_size = c.marker_normal_scale.w * 0.01 * view_height;
    return origin_relative + axial + planar * world_size;
}

@vertex fn vs_main(@builtin(vertex_index) vid: u32, in: InstanceIn) -> VertexOut {
    let c = wire_consts[in.wire_id];

    let which_end_arr = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let side_arr      = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let which_end = which_end_arr[vid];
    let side      = side_arr[vid];

    let rel_a = marker_relative(in.pos_a, in.pos_a_low, c);
    let rel_b = marker_relative(in.pos_b, in.pos_b_low, c);
    let is_world = c.world_half_width > 0.0 || in.taper_ratio.x > 0.0 || in.taper_ratio.y > 0.0;

    var final_clip: vec4<f32>;
    var out_dist: f32;
    var out_cap: vec2<f32>;
    var out_cap_ends: vec3<f32>;

    if is_world {
        // Wide polylines have physical world-unit width (world_half_width or taper > 0).
        // Expand the quad in 3D world space on the entity's plane (perpendicular to
        // the segment and the plane normal), so the rectangle remains hosted rigidly
        // on the 3D plane when the camera rotates, orbits, or tilts, matching CAD
        // behavior and circle.wgsl planar arcs.
        let world_hw_a = select(c.world_half_width, in.taper_ratio.x * c.world_half_width, in.taper_ratio.x > 0.0);
        let world_hw_b = select(c.world_half_width, in.taper_ratio.y * c.world_half_width, in.taper_ratio.y > 0.0);
        let cur_world_hw = mix(world_hw_a, world_hw_b, which_end);
        let eff_hw = max(cur_world_hw, 0.5 * u.world_per_pixel);

        let world_delta = rel_b - rel_a;
        let world_len = length(world_delta);
        var world_dir = vec3<f32>(1.0, 0.0, 0.0);
        if world_len > 1e-6 {
            world_dir = world_delta / world_len;
        }

        var norm = vec3<f32>(0.0, 0.0, 1.0);
        if length(c.marker_normal_scale.xyz) > 1e-4 {
            norm = normalize(c.marker_normal_scale.xyz);
        }

        var perp_world = cross(norm, world_dir);
        if length(perp_world) < 1e-4 {
            perp_world = cross(vec3<f32>(0.0, 1.0, 0.0), world_dir);
            if length(perp_world) < 1e-4 {
                perp_world = cross(vec3<f32>(1.0, 0.0, 0.0), world_dir);
            }
        }
        perp_world = normalize(perp_world);

        let pos_rel = mix(rel_a, rel_b, which_end);
        let world_pos = pos_rel + perp_world * (eff_hw * side);
        var clip_pos = u.view_rot * vec4<f32>(world_pos, 1.0);
        clip_pos.z = clip_pos.z - c.draw_depth * DRAW_ORDER_BIAS * clip_pos.w;

        final_clip = clip_pos;
        out_dist = mix(in.distance_a, in.distance_b, which_end);
        out_cap = vec2<f32>(which_end * world_len, eff_hw * side);
        out_cap_ends = vec3<f32>(world_len, world_hw_a, world_hw_b);
    } else {
        // Thin wires expand in screen pixels facing the camera with rounded end caps / joints.
        let clip_a = u.view_rot * vec4<f32>(rel_a, 1.0);
        let clip_b = u.view_rot * vec4<f32>(rel_b, 1.0);

        let ndc_a = clip_a.xy / clip_a.w;
        let ndc_b = clip_b.xy / clip_b.w;

        let screen_a = ndc_a * u.viewport_size * 0.5;
        let screen_b = ndc_b * u.viewport_size * 0.5;

        let seg = screen_b - screen_a;
        let seg_len = length(seg);
        var dir: vec2<f32>;
        if seg_len > 1e-4 {
            dir = seg / seg_len;
        } else {
            dir = vec2<f32>(1.0, 0.0);
        }
        let perp = vec2<f32>(-dir.y, dir.x);

        let clip_pos = mix(clip_a, clip_b, which_end);

        let hw = resolve_hw(0.0, 0.0, c.half_width);
        let ext = which_end * 2.0 - 1.0;
        let offset_px = perp * hw * side + dir * hw * ext;
        let ndc_offset = offset_px / (u.viewport_size * 0.5);
        var clip_pos_out = clip_pos + vec4<f32>(ndc_offset * clip_pos.w, 0.0, 0.0);
        clip_pos_out.z = clip_pos_out.z - c.draw_depth * DRAW_ORDER_BIAS * clip_pos_out.w;

        final_clip = clip_pos_out;
        out_dist = mix(in.distance_a, in.distance_b, which_end) + ext * hw * u.world_per_pixel;
        out_cap = vec2<f32>(which_end * seg_len + ext * hw, hw * side);
        out_cap_ends = vec3<f32>(seg_len, hw, hw);
    }

    let lt_scale = u.linetype_scale;
    var min_elem: f32 = c.pattern_length * lt_scale;
    let elems = array<f32, 8>(
        c.pat0.x * lt_scale, c.pat0.y * lt_scale,
        c.pat0.z * lt_scale, c.pat0.w * lt_scale,
        c.pat1.x * lt_scale, c.pat1.y * lt_scale,
        c.pat1.z * lt_scale, c.pat1.w * lt_scale,
    );
    for (var i = 0u; i < 8u; i++) {
        let e = abs(elems[i]);
        if e > 0.0 && e < min_elem { min_elem = e; }
    }

    var out: VertexOut;
    out.clip_pos       = final_clip;
    out.color          = c.color;
    out.distance       = out_dist;
    out.cap            = out_cap;
    out.cap_ends       = out_cap_ends;
    out.pattern_length = c.pattern_length * lt_scale;
    out.pat0           = c.pat0 * lt_scale;
    out.pat1           = c.pat1 * lt_scale;
    out.min_elem       = min_elem;
    out.align_end      = c.align_end * lt_scale;
    out.align_total    = c.align_total;
    return out;
}

fn in_dash(dist: f32, pat_len: f32, p0: vec4<f32>, p1: vec4<f32>, align_end: f32, align_total: f32) -> bool {
    let elems = array<f32, 8>(p0.x, p0.y, p0.z, p0.w, p1.x, p1.y, p1.z, p1.w);
    var count = 0u;
    for (var i = 0u; i < 8u; i++) {
        if elems[i] != 0.0 { count = i + 1u; }
    }

    var d: f32;
    if align_total > 0.0 {
        // "A"-type alignment: the line begins and ends with a solid dash of
        // length `align_end`. Force the two end regions lit, then phase the
        // interior so the element AFTER the first dash resumes exactly at
        // `align_end` (the interior meets each end dash on a gap boundary).
        if dist <= align_end || dist >= align_total - align_end {
            return true;
        }
        var first_dash = 0.0;
        for (var i = 0u; i < count; i++) {
            if elems[i] > 0.0 { first_dash = elems[i]; break; }
        }
        d = ((dist - align_end + first_dash) % pat_len + pat_len) % pat_len;
    } else {
        d = ((dist % pat_len) + pat_len) % pat_len;
    }

    var pos = 0.0f;
    let dot_half = u.world_per_pixel * 0.75;
    for (var i = 0u; i < count; i++) {
        let elem = elems[i];
        if elem == 0.0 {
            let dd = abs(d - pos);
            if min(dd, pat_len - dd) <= dot_half { return true; }
        } else if elem > 0.0 {
            if d >= pos && d < pos + elem { return true; }
            pos += elem;
        } else {
            pos += -elem;
        }
    }
    return false;
}

// Round the cap overhang off: outside the segment span only pixels within
// the end's half-width radius survive, giving round joints and end caps.
fn cap_clipped(cap: vec2<f32>, cap_ends: vec3<f32>) -> bool {
    if cap.x < 0.0 {
        return length(cap) > cap_ends.y;
    }
    if cap.x > cap_ends.x {
        return length(vec2<f32>(cap.x - cap_ends.x, cap.y)) > cap_ends.z;
    }
    return false;
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // Negative pattern length is the persistent-arena tombstone sentinel.
    // Discard before cap/alpha work so deleted slabs cannot write color/depth.
    if in.pattern_length < 0.0 {
        discard;
    }
    if cap_clipped(in.cap, in.cap_ends) {
        discard;
    }
    if in.pattern_length > 0.0 {
        if in.min_elem >= u.world_per_pixel {
            if !in_dash(in.distance, in.pattern_length, in.pat0, in.pat1, in.align_end, in.align_total) {
                discard;
            }
        }
    }
    let alpha = select(1.0, in.color.a, u.transparency_enable > 0.5);
    return vec4<f32>(in.color.rgb, alpha);
}

// Black variant: used for 3D mesh outline edges in filled render modes so the
// mesh reads as a shaded surface framed by black edges. Keeps the dash/LOD
// logic identical to `fs_main`; only the RGB is forced to black.
@fragment fn fs_black(in: VertexOut) -> @location(0) vec4<f32> {
    if in.pattern_length < 0.0 {
        discard;
    }
    if cap_clipped(in.cap, in.cap_ends) {
        discard;
    }
    if in.pattern_length > 0.0 {
        if in.min_elem >= u.world_per_pixel {
            if !in_dash(in.distance, in.pattern_length, in.pat0, in.pat1, in.align_end, in.align_total) {
                discard;
            }
        }
    }
    let alpha = select(1.0, in.color.a, u.transparency_enable > 0.5);
    return vec4<f32>(0.0, 0.0, 0.0, alpha);
}
