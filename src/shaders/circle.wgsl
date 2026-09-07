// GPU-instanced analytical circle shader — renders 2D/planar circles as screen/plane-aligned quads.
// Topology: TriangleList, 6 vertices drawn per INSTANCE.
//
// One instance = one complete circle. The six vertex IDs map to the corners of a
// two-triangle quad covering the circle's bounding box (+ lineweight margin).
//
// The fragment shader analytically computes the exact distance to the circle perimeter:
//   d_world = abs(length(local_xy) - radius)
// and uses screen-space derivatives (fwidth) to achieve perfect sub-pixel anti-aliasing
// at any zoom level, without any polygon faceting or CPU re-tessellation.

struct Uniforms {
    viewport_size:       vec2<f32>,
    world_per_pixel:     f32,
    lwdisplay_enable:    f32,
    flat_shade:          f32,
    transparency_enable: f32,
    linetype_scale:      f32,
    lineweight_scale:    f32,
    // Relative-to-eye (double-single)
    view_rot:            mat4x4<f32>,
    eye_high:            vec3<f32>,
    _pad_eh:             f32,
    eye_low:             vec3<f32>,
    _pad_el:             f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

const DRAW_ORDER_BIAS: f32 = 0.001;
const MODEL_LINEWEIGHT_BOOST: f32 = 2.0;
const MODEL_LINEWEIGHT_MAX_PX: f32 = 10.0;
const TAU: f32 = 6.283185307179586;

struct InstanceIn {
    @location(0) center_high: vec4<f32>, // xyz = center_high, w = unused
    @location(1) center_low:  vec4<f32>, // xyz = center_low, w = radius
    @location(2) axis_x:      vec4<f32>, // xyz = axis_x, w = start_angle
    @location(3) axis_y:      vec4<f32>, // xyz = axis_y, w = end_angle
    @location(4) color:       vec4<f32>, // rgba
    @location(5) params:      vec4<f32>, // half_width_px, pattern_length, draw_depth, unused
    @location(6) pat0:        vec4<f32>,
    @location(7) pat1:        vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_pos:       vec4<f32>,
    @location(0)       local_pos:      vec2<f32>,
    @location(1)       radius:         f32,
    @location(2)       hw_px:          f32,
    @location(3)       color:          vec4<f32>,
    @location(4)       pattern_length: f32,
    @location(5)       pat0:           vec4<f32>,
    @location(6)       pat1:           vec4<f32>,
    @location(7) @interpolate(flat) min_elem:       f32,
    @location(8) @interpolate(flat) start_angle:    f32,
    @location(9) @interpolate(flat) end_angle:      f32,
}

fn resolve_hw(px_hw: f32) -> f32 {
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

@vertex fn vs_main(@builtin(vertex_index) vid: u32, in: InstanceIn) -> VertexOut {
    // Two-triangle unit quad corner table:
    //   vid 0,1,2 = (-1,-1) ( 1,-1) ( 1, 1)
    //   vid 3,4,5 = (-1,-1) ( 1, 1) (-1, 1)
    let u_arr = array<f32, 6>(-1.0,  1.0,  1.0, -1.0,  1.0, -1.0);
    let v_arr = array<f32, 6>(-1.0, -1.0,  1.0, -1.0,  1.0,  1.0);
    let u_val = u_arr[vid];
    let v_val = v_arr[vid];

    let center_high = in.center_high.xyz;
    let center_low = in.center_low.xyz;
    let radius = in.center_low.w;
    let axis_x = in.axis_x.xyz;
    let axis_y = in.axis_y.xyz;
    let color = in.color;
    let hw_px = resolve_hw(in.params.x);
    let pattern_length = in.params.y;
    let draw_depth = in.params.z;

    // Expand bounding quad so anti-aliasing margin and lineweight never clip
    let margin_world = (hw_px + 2.0) * max(u.world_per_pixel, 1e-6);
    let ext = radius + margin_world;

    let center_rel = (center_high - u.eye_high) + (center_low - u.eye_low);
    let world_pos_rel = center_rel + (u_val * axis_x + v_val * axis_y) * ext;

    var clip_pos = u.view_rot * vec4<f32>(world_pos_rel, 1.0);
    clip_pos.z = clip_pos.z - draw_depth * DRAW_ORDER_BIAS * clip_pos.w;

    var out: VertexOut;
    out.clip_pos = clip_pos;
    out.local_pos = vec2<f32>(u_val * ext, v_val * ext);
    out.radius = radius;
    out.hw_px = hw_px;
    out.color = color;
    out.pattern_length = pattern_length;
    out.pat0 = in.pat0;
    out.pat1 = in.pat1;

    // Compute smallest non-zero pattern element for LOD short-circuiting
    let elems = array<f32, 8>(in.pat0.x, in.pat0.y, in.pat0.z, in.pat0.w, in.pat1.x, in.pat1.y, in.pat1.z, in.pat1.w);
    var min_elem = 1e9;
    for (var i = 0u; i < 8u; i++) {
        let e = abs(elems[i]);
        if e > 0.0 && e < min_elem {
            min_elem = e;
        }
    }
    out.min_elem = select(0.0, min_elem, min_elem < 1e8);

    out.start_angle = in.axis_x.w;
    out.end_angle = in.axis_y.w;

    return out;
}

fn mod_tau(val: f32) -> f32 {
    let m = val % TAU;
    return select(m, m + TAU, m < 0.0);
}

fn in_dash(dist: f32, pat_len: f32, p0: vec4<f32>, p1: vec4<f32>) -> bool {
    let elems = array<f32, 8>(p0.x, p0.y, p0.z, p0.w, p1.x, p1.y, p1.z, p1.w);
    var count = 0u;
    for (var i = 0u; i < 8u; i++) {
        if elems[i] != 0.0 { count = i + 1u; }
    }
    if count == 0u {
        return true;
    }

    let d = ((dist % pat_len) + pat_len) % pat_len;
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

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let r = length(in.local_pos);

    // Screen-space derivative anti-aliasing:
    // Use the gradient of the monotonic radial distance `r` instead of `abs(r - radius)`.
    // Taking derivatives across the cusp of abs() cancels out at scanline crossings,
    // which previously collapsed fw to near-zero and punched periodic 1-2px holes (false dashes).
    let grad = vec2<f32>(dpdx(r), dpdy(r));
    let grad_len = length(grad);
    let fw = max(select(u.world_per_pixel, grad_len, grad_len > 1e-6), 1e-6);

    let sa = in.start_angle;
    let ea = in.end_angle;
    var sweep = mod_tau(ea - sa);
    if sweep <= 1e-5 && abs(ea - sa) > 1e-5 {
        sweep = TAU;
    }

    let angle = atan2(in.local_pos.y, in.local_pos.x);
    let theta = select(angle, angle + TAU, angle < 0.0);
    let d_theta = mod_tau(theta - sa);

    var d_world: f32;
    var arc_dist: f32;

    if sweep >= TAU - 1e-5 || d_theta <= sweep {
        // Inside arc sweep or full circle
        d_world = abs(r - in.radius);
        arc_dist = in.radius * d_theta;
    } else {
        // Outside sweep: measure distance to start and end endpoints for round end caps
        let p_start = in.radius * vec2<f32>(cos(sa), sin(sa));
        let p_end = in.radius * vec2<f32>(cos(ea), sin(ea));
        let d_start = length(in.local_pos - p_start);
        let d_end = length(in.local_pos - p_end);
        if d_start < d_end {
            d_world = d_start;
            arc_dist = 0.0;
        } else {
            d_world = d_end;
            arc_dist = in.radius * sweep;
        }
    }

    let d_px = d_world / fw;
    let alpha_cov = clamp(0.5 + in.hw_px - d_px, 0.0, 1.0);
    if alpha_cov <= 0.0 {
        discard;
    }

    // Linetype dash pattern test
    if in.pattern_length > 0.0 {
        let pat_len = in.pattern_length * u.linetype_scale;
        if in.min_elem * u.linetype_scale >= u.world_per_pixel && pat_len > 0.0 {
            if !in_dash(arc_dist, pat_len, in.pat0, in.pat1) {
                discard;
            }
        }
    }

    let alpha = select(1.0, in.color.a, u.transparency_enable > 0.5) * alpha_cov;
    return vec4<f32>(in.color.rgb, alpha);
}
