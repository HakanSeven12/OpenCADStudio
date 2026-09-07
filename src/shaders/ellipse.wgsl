// GPU-instanced analytical ellipse shader — renders 2D/planar ellipses and elliptical arcs as quads.
// Topology: TriangleList, 6 vertices drawn per INSTANCE.
//
// Evaluates Inigo Quilez's exact 2D Ellipse Signed Distance Function (quartic closest-point solver)
// with sub-pixel anti-aliasing and parametric arc endpoint clamping for round end caps.

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
    @location(1) center_low:  vec4<f32>, // xyz = center_low, w = minor_axis_ratio
    @location(2) major_axis:  vec4<f32>, // xyz = major_axis (semi-major vector), w = start_param
    @location(3) normal:      vec4<f32>, // xyz = normal, w = end_param
    @location(4) color:       vec4<f32>, // rgba
    @location(5) params:      vec4<f32>, // half_width_px, pattern_length, draw_depth, unused
    @location(6) pat0:        vec4<f32>,
    @location(7) pat1:        vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip_pos:       vec4<f32>,
    @location(0)       local_pos:      vec2<f32>,
    @location(1)       semi_axes:      vec2<f32>,
    @location(2)       hw_px:          f32,
    @location(3)       color:          vec4<f32>,
    @location(4)       pattern_length: f32,
    @location(5)       pat0:           vec4<f32>,
    @location(6)       pat1:           vec4<f32>,
    @location(7) @interpolate(flat) min_elem:    f32,
    @location(8) @interpolate(flat) start_param: f32,
    @location(9) @interpolate(flat) end_param:   f32,
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
    let u_arr = array<f32, 6>(-1.0,  1.0,  1.0, -1.0,  1.0, -1.0);
    let v_arr = array<f32, 6>(-1.0, -1.0,  1.0, -1.0,  1.0,  1.0);
    let u_val = u_arr[vid];
    let v_val = v_arr[vid];

    let center_high = in.center_high.xyz;
    let center_low = in.center_low.xyz;
    let major_vec = in.major_axis.xyz;
    let norm_vec = in.normal.xyz;

    let a = length(major_vec);
    let u_axis = select(vec3<f32>(1.0, 0.0, 0.0), major_vec / a, a > 1e-6);
    let n_len = length(norm_vec);
    let n_axis = select(vec3<f32>(0.0, 0.0, 1.0), norm_vec / n_len, n_len > 1e-6);
    let v_axis = normalize(cross(n_axis, u_axis));

    let ratio = clamp(in.center_low.w, 1e-6, 1.0);
    let b = a * ratio;

    let color = in.color;
    let hw_px = resolve_hw(in.params.x);
    let pattern_length = in.params.y;
    let draw_depth = in.params.z;

    let margin_world = (hw_px + 2.0) * max(u.world_per_pixel, 1e-6);
    let ext_u = a + margin_world;
    let ext_v = b + margin_world;

    let center_rel = (center_high - u.eye_high) + (center_low - u.eye_low);
    let world_pos_rel = center_rel + (u_val * ext_u) * u_axis + (v_val * ext_v) * v_axis;

    var clip_pos = u.view_rot * vec4<f32>(world_pos_rel, 1.0);
    clip_pos.z = clip_pos.z - draw_depth * DRAW_ORDER_BIAS * clip_pos.w;

    var out: VertexOut;
    out.clip_pos = clip_pos;
    out.local_pos = vec2<f32>(u_val * ext_u, v_val * ext_v);
    out.semi_axes = vec2<f32>(a, b);
    out.hw_px = hw_px;
    out.color = color;
    out.pattern_length = pattern_length;
    out.pat0 = in.pat0;
    out.pat1 = in.pat1;
    out.start_param = in.major_axis.w;
    out.end_param = in.normal.w;

    let elems = array<f32, 8>(in.pat0.x, in.pat0.y, in.pat0.z, in.pat0.w, in.pat1.x, in.pat1.y, in.pat1.z, in.pat1.w);
    var min_elem = 1e9;
    for (var i = 0u; i < 8u; i++) {
        let e = abs(elems[i]);
        if e > 0.0 && e < min_elem {
            min_elem = e;
        }
    }
    out.min_elem = select(0.0, min_elem, min_elem < 1e8);

    return out;
}

fn mod_tau(val: f32) -> f32 {
    let m = val % TAU;
    return select(m, m + TAU, m < 0.0);
}

struct ClosestResult {
    q: vec2<f32>,
    t: f32,
}

fn closest_point_ellipse(p_in: vec2<f32>, ab: vec2<f32>) -> ClosestResult {
    let px = abs(p_in.x);
    let py = abs(p_in.y);
    let a = ab.x;
    let b = ab.y;
    if abs(a - b) < 1e-5 {
        let len = length(p_in);
        let q_circ = select(p_in, normalize(p_in) * a, len > 1e-6);
        let ang = atan2(q_circ.y, q_circ.x);
        let full_t = select(ang, ang + TAU, ang < 0.0);
        return ClosestResult(q_circ, full_t);
    }

    // Initial estimate from eccentric anomaly of radial projection
    var t = atan2(py * a, px * b);
    let k = a * a - b * b;

    // 3 iterations of Newton-Raphson for machine precision
    for (var i = 0u; i < 3u; i++) {
        let st = sin(t);
        let ct = cos(t);
        let f = k * st * ct - px * a * st + py * b * ct;
        let f_prime = k * (ct * ct - st * st) - px * a * ct - py * b * st;
        if abs(f_prime) > 1e-7 {
            t -= f / f_prime;
        }
    }
    t = clamp(t, 0.0, 1.5707963267948966);

    let q_quad = vec2<f32>(a * cos(t), b * sin(t));
    let q = vec2<f32>(
        select(-q_quad.x, q_quad.x, p_in.x >= 0.0),
        select(-q_quad.y, q_quad.y, p_in.y >= 0.0)
    );

    var full_t = t;
    if (p_in.x < 0.0) {
        full_t = 3.141592653589793 - t;
    }
    if (p_in.y < 0.0) {
        full_t = select(TAU - t, 3.141592653589793 + t, p_in.x < 0.0);
    }

    return ClosestResult(q, full_t);
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
    let ab = in.semi_axes;
    let res = closest_point_ellipse(in.local_pos, ab);
    let q = res.q;
    let t = res.t;

    let sa = in.start_param;
    let ea = in.end_param;
    var sweep = mod_tau(ea - sa);
    if sweep <= 1e-5 && abs(ea - sa) > 1e-5 {
        sweep = TAU;
    }

    let d_t = mod_tau(t - sa);

    var d_world: f32;
    var arc_dist: f32;

    // Ramanujan approximation of ellipse perimeter for dash scaling
    let h_val = ((ab.x - ab.y) * (ab.x - ab.y)) / max((ab.x + ab.y) * (ab.x + ab.y), 1e-6);
    let perimeter = 3.141592653589793 * (ab.x + ab.y) * (1.0 + (3.0 * h_val) / max(10.0 + sqrt(max(4.0 - 3.0 * h_val, 0.0)), 1e-6));

    if sweep >= TAU - 1e-5 || d_t <= sweep {
        d_world = length(in.local_pos - q);
        arc_dist = perimeter * (d_t / TAU);
    } else {
        let p_start = vec2<f32>(ab.x * cos(sa), ab.y * sin(sa));
        let p_end = vec2<f32>(ab.x * cos(ea), ab.y * sin(ea));
        let d_start = length(in.local_pos - p_start);
        let d_end = length(in.local_pos - p_end);
        if d_start < d_end {
            d_world = d_start;
            arc_dist = 0.0;
        } else {
            d_world = d_end;
            arc_dist = perimeter * (sweep / TAU);
        }
    }

    // Isotropic quad screen-space filter width:
    // Deriving from interpolated linear coordinates eliminates scanline derivative cancellations
    let dx = dpdx(in.local_pos);
    let dy = dpdy(in.local_pos);
    let fw_len = sqrt(0.5 * (dot(dx, dx) + dot(dy, dy)));
    let fw = max(select(u.world_per_pixel, fw_len, fw_len > 1e-6), 1e-6);

    let d_px = d_world / fw;
    let alpha_cov = clamp(0.5 + in.hw_px - d_px, 0.0, 1.0);
    if alpha_cov <= 0.0 {
        discard;
    }

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
