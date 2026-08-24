// Wire shader — renders 1-D CAD entities as screen-aligned quads.
// Topology: TriangleList, 6 vertices drawn per INSTANCE.
//
// One instance = one segment. The six vertex IDs map to the corners of a
// two-triangle quad; the vertex shader derives `which_end` (0=A end, 1=B end)
// and `side` (±1 perpendicular) from `@builtin(vertex_index)` and expands the
// quad by `half_width` pixels perpendicular to the segment direction in
// screen space.
//
// Linetype is applied entirely on the GPU:
//   • distance = cumulative arc-length, linearly interpolated from
//     (distance_a, distance_b) by `which_end`.
//   • pattern_length > 0 enables the dash test; 0 = solid (no discard).
//   • pat0/pat1 encode up to 8 elements: positive=dash, negative=gap,
//     exactly 0=dot (rendered as a fixed ~1 px mark). Trailing 0.0 slots are
//     padding; the real element count is (index of last non-zero) + 1.

struct Uniforms {
    viewport_size:    vec2<f32>,
    world_per_pixel:  f32,
    // LWDISPLAY toggle: 0.0 = force 1 px (half_width 0.5), 1.0 = use the
    // per-instance baked half_width. Lets the LWT button switch without
    // retessellating.
    lwdisplay_enable: f32,
    // Mesh flat-shade flag (unused here; kept so the field offsets match
    // the shared Uniforms buffer layout).
    flat_shade: f32,
    // Transparency-display toggle: 1.0 = honour baked alpha, 0.0 = force
    // every line opaque.
    transparency_enable: f32,
    linetype_scale: f32,
    _pad: f32,
    // ── Relative-to-eye (double-single) ──────────────────────────────────
    // view_rot is the rotation-only view-projection; vertices subtract the eye
    // (eye_high + eye_low, two f32 emulating f64) before transforming, so the
    // large eye translation never enters the f32 matrix → no large-coordinate
    // jitter on pan / zoom / rotate.
    view_rot:         mat4x4<f32>,
    eye_high:         vec3<f32>,
    _pad_eh:          f32,
    eye_low:          vec3<f32>,
    _pad_el:          f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// Scalars are packed into vec4 attributes: WebGL2 / WebGPU cap vertex
// attributes at 16, and the unpacked layout had grown to 17 — the wire
// pipeline failed to build and the web viewport drew no lines at all (#414).
struct InstanceIn {
    @location(0) pos_a:          vec3<f32>,
    @location(1) pos_b:          vec3<f32>,
    @location(2) color:          vec4<f32>,
    // distance_a, distance_b, half_width, pattern_length
    @location(3) dists:          vec4<f32>,
    @location(4) pat0:           vec4<f32>,
    @location(5) pat1:           vec4<f32>,
    // draw_depth, align_end ("A"-type end-dash length), align_total (total
    // wire length), world_half_width (wide-polyline band; 0 = normal wire)
    @location(6) misc:           vec4<f32>,
    // Double-single low residuals of the endpoints.
    @location(7) pos_a_low:      vec3<f32>,
    @location(8) pos_b_low:      vec3<f32>,
    // Per-endpoint world half-width for a tapered band (0 = use the constant).
    @location(9) taper:          vec2<f32>,
    @location(10) marker_origin_high: vec4<f32>,
    @location(11) marker_origin_low: vec4<f32>,
    @location(12) marker_normal_scale: vec4<f32>,
}

// Draw-order depth bias: shifts clip-space z so 2D entities of different
// types order against each other through the shared LessEqual depth test.
// draw_depth is signed (-1,1): front → positive → smaller z → drawn on top;
// 0.0 = neutral (real depth). Depth32Float gives ample precision.
const DRAW_ORDER_BIAS: f32 = 0.001;

struct VertexOut {
    @builtin(position)              clip_pos:       vec4<f32>,
    @location(0)                    color:          vec4<f32>,
    @location(1)                    distance:       f32,
    @location(2)                    pattern_length: f32,
    @location(3)                    pat0:           vec4<f32>,
    @location(4)                    pat1:           vec4<f32>,
    // World length of the smallest non-zero dash / gap element of this
    // instance. Flat-interpolated (constant per instance) so the
    // fragment stage can short-circuit the dash test when every gap
    // projects below one pixel on screen. See the LOD branch in
    // `fs_main`.
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
fn resolve_hw(taper: f32, world_hw: f32, px_hw: f32) -> f32 {
    if taper > 0.0 { return max(taper / u.world_per_pixel, 0.5); }
    if world_hw > 0.0 { return max(world_hw / u.world_per_pixel, 0.5); }
    return select(0.5, px_hw, u.lwdisplay_enable > 0.5);
}

fn marker_relative(position_high: vec3<f32>, position_low: vec3<f32>, instance: InstanceIn) -> vec3<f32> {
    if instance.marker_normal_scale.w <= 0.0 {
        return (position_high - u.eye_high) + (position_low - u.eye_low);
    }
    let origin_high = instance.marker_origin_high.xyz;
    let origin_low = instance.marker_origin_low.xyz;
    let origin_relative = (origin_high - u.eye_high) + (origin_low - u.eye_low);
    let delta = (position_high - origin_high) + (position_low - origin_low);
    let normal = normalize(instance.marker_normal_scale.xyz);
    let axial = normal * dot(delta, normal);
    let planar = delta - axial;
    let origin_clip = u.view_rot * vec4<f32>(origin_relative, 1.0);
    let projection_scale = max(length(vec3<f32>(
        u.view_rot[0].y,
        u.view_rot[1].y,
        u.view_rot[2].y,
    )), 1e-12);
    let view_height = 2.0 * max(abs(origin_clip.w), 1e-6) / projection_scale;
    let world_size = instance.marker_normal_scale.w * 0.01 * view_height;
    return origin_relative + axial + planar * world_size;
}

@vertex fn vs_main(@builtin(vertex_index) vid: u32, in: InstanceIn) -> VertexOut {
    // Two-triangle quad corner table:
    //   vid 0,1,2 = (A,-1) (B,-1) (B,+1)
    //   vid 3,4,5 = (A,-1) (B,+1) (A,+1)
    let which_end_arr = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let side_arr      = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let which_end = which_end_arr[vid];
    let side      = side_arr[vid];

    // Double-single relative-to-eye: subtract the eye from each endpoint with
    // both halves of the f64-emulating pair, then transform by the rotation-only
    // view-projection. (pos_high − eye_high) is exact in f32 for same-magnitude
    // operands (Sterbenz); adding (pos_low − eye_low) restores the residual both
    // the vertex and the eye would otherwise lose — so geometry stays put at
    // UTM-scale coordinates and after a cross-drawing paste, with no jitter.
    let rel_a = marker_relative(in.pos_a, in.pos_a_low, in);
    let rel_b = marker_relative(in.pos_b, in.pos_b_low, in);
    let clip_a = u.view_rot * vec4<f32>(rel_a, 1.0);
    let clip_b = u.view_rot * vec4<f32>(rel_b, 1.0);

    // NDC of both endpoints.
    let ndc_a = clip_a.xy / clip_a.w;
    let ndc_b = clip_b.xy / clip_b.w;

    // Screen-space pixel positions.
    let screen_a = ndc_a * u.viewport_size * 0.5;
    let screen_b = ndc_b * u.viewport_size * 0.5;

    // Screen-space direction / perpendicular of the segment.
    let seg = screen_b - screen_a;
    let seg_len = length(seg);
    var dir: vec2<f32>;
    if seg_len > 1e-4 {
        dir = seg / seg_len;
    } else {
        dir = vec2<f32>(1.0, 0.0);
    }
    let perp = vec2<f32>(-dir.y, dir.x);

    // Select the clip-space position for this vertex's endpoint.
    let clip_pos = mix(clip_a, clip_b, which_end);

    // A wide polyline carries its band width in world units: expand the quad by
    // `world_half_width / world_per_pixel` (pixels) so the band tracks zoom. A
    // normal wire (world_half_width == 0) uses the screen-pixel half-width,
    // honouring the LWDISPLAY toggle (off → collapse to a 1-pixel line).
    // A tapered band interpolates a per-endpoint world half-width across the
    // segment; a constant band uses `world_half_width`. Both clamp to a
    // half-pixel so a zoomed-out band stays a hairline instead of vanishing.
    let hw_a = resolve_hw(in.taper.x, in.misc.w, in.dists.z);
    let hw_b = resolve_hw(in.taper.y, in.misc.w, in.dists.z);
    let hw = mix(hw_a, hw_b, which_end);

    // Extend the quad longitudinally by the end half-width and let the
    // fragment stage round the overhang off: adjoining segments then meet in
    // overlapping round joints, closing the wedge gaps a perpendicular-only
    // expansion leaves on the outside of corners and along tessellated arcs.
    let ext = which_end * 2.0 - 1.0; // -1 at the A end, +1 at the B end
    let offset_px = perp * hw * side + dir * hw * ext;
    let ndc_offset = offset_px / (u.viewport_size * 0.5);
    let final_clip = clip_pos + vec4<f32>(ndc_offset * clip_pos.w, 0.0, 0.0);

    // Smallest non-zero dash / gap element, in world units. Used by
    // the fragment stage to decide when the pattern's finest feature
    // would render below one pixel and should collapse to a solid line.
    let lt_scale = u.linetype_scale;
    var min_elem: f32 = in.dists.w * lt_scale;
    let elems = array<f32, 8>(
        in.pat0.x * lt_scale, in.pat0.y * lt_scale,
        in.pat0.z * lt_scale, in.pat0.w * lt_scale,
        in.pat1.x * lt_scale, in.pat1.y * lt_scale,
        in.pat1.z * lt_scale, in.pat1.w * lt_scale,
    );
    for (var i = 0u; i < 8u; i++) {
        let e = abs(elems[i]);
        if e > 0.0 && e < min_elem { min_elem = e; }
    }

    var out: VertexOut;
    out.clip_pos       = final_clip;
    out.clip_pos.z     = out.clip_pos.z - in.misc.x * DRAW_ORDER_BIAS * out.clip_pos.w;
    out.color          = in.color;
    // Dash arc-length, extrapolated over the cap overhang so the pattern
    // stays continuous through a joint.
    out.distance       = mix(in.dists.x, in.dists.y, which_end)
        + ext * hw * u.world_per_pixel;
    out.cap            = vec2<f32>(which_end * seg_len + ext * hw, hw * side);
    out.cap_ends       = vec3<f32>(seg_len, hw_a, hw_b);
    out.pattern_length = in.dists.w * lt_scale;
    out.pat0           = in.pat0 * lt_scale;
    out.pat1           = in.pat1 * lt_scale;
    out.min_elem       = min_elem;
    out.align_end      = in.misc.y * lt_scale;
    out.align_total    = in.misc.z;
    return out;
}

// Returns true if arc-length `dist` falls inside a dash or on a dot.
fn in_dash(dist: f32, pat_len: f32, p0: vec4<f32>, p1: vec4<f32>, align_end: f32, align_total: f32) -> bool {
    let elems = array<f32, 8>(p0.x, p0.y, p0.z, p0.w, p1.x, p1.y, p1.z, p1.w);
    // Real element count = (index of last non-zero) + 1. Trailing 0.0 slots
    // are padding; a 0.0 within this range is a real dot.
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
    // A dot is a zero-length element: render it as a fixed ~1.5 px mark
    // (half-width ~0.75 px in world units) so it stays visible at any zoom
    // instead of vanishing with its zero world-length. Mirrors the hatch
    // shader's pixel-snapped dot. (#149)
    let dot_half = u.world_per_pixel * 0.75;
    for (var i = 0u; i < count; i++) {
        let elem = elems[i];
        if elem == 0.0 {
            // Dot centred at `pos` (zero length); light a small mark, wrapped
            // around the pattern so a dot at 0 also covers the seam at pat_len.
            let dd = abs(d - pos);
            if min(dd, pat_len - dd) <= dot_half { return true; }
        } else if elem > 0.0 {
            if d >= pos && d < pos + elem { return true; }   // inside a dash
            pos += elem;
        } else {
            pos += -elem;                                    // skip a gap
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
        // LOD: once the pattern's smallest feature drops below ~1 px
        // on screen, dash gaps alias / shimmer (or vanish completely)
        // and the user reads the line as solid anyway. Skip the dash
        // test and return solid colour — also saves the per-fragment
        // arc-length math + `discard`.
        if in.min_elem >= u.world_per_pixel {
            if !in_dash(in.distance, in.pattern_length, in.pat0, in.pat1, in.align_end, in.align_total) {
                discard;
            }
        }
    }
    // Transparency display off → force the line opaque.
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
