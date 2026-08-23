// Hatch shader — GPU-driven hatch fill over an arbitrary polygon boundary.
//
// Vertex shader  : projects a bounding-box quad over the hatch region.
// Fragment shader: three stages —
//   1. Point-in-polygon (ray casting against boundary vertices in world XY).
//   2. Pattern test:
//        mode 0 — N line families (PAT format), each with optional dash pattern.
//        mode 1 — solid fill.
//        mode 2 — linear gradient.
//
// GPU limits: MAX_FAMILIES = 16, MAX_DASHES = 128.

// ── Group 0: frame uniforms (shared) ──────────────────────────────────────

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
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Group 1: per-hatch data ────────────────────────────────────────────────
//
// mode encoding:
//   0 → Pattern  (evaluate FamilyBatch)
//   1 → Solid    (return h.color immediately)
//   2 → Gradient (mix h.color → h.color2 along grad_cos/grad_sin)

struct HatchUniforms {
    color:        vec4<f32>,  //  0: primary RGBA
    color2:       vec4<f32>,  // 16: gradient end color
    mode:         u32,        // 32: 0=pattern, 1=solid, 2=gradient
    vcount:       u32,        // 36: boundary vertex count
    angle_offset: f32,        // 40: pattern rotation (radians, added to each family)
    scale:        f32,        // 44: pattern scale multiplier
    grad_cos:     f32,        // 48: gradient direction cos
    grad_sin:     f32,        // 52: gradient direction sin
    grad_min:     f32,        // 56: gradient proj_min
    grad_range:   f32,        // 60: gradient proj_range
    origin:       vec2<f32>,  // 64: reserved pattern origin
    origin_low:   vec2<f32>,  // 72: reserved low residual
    draw_depth:   f32,        // 80: signed (-1,1) draw-order bias; 0 = neutral
    _pad0:        f32,        // 84
    _pad1:        vec2<f32>,  // 88
    plane_origin_high: vec4<f32>,
    plane_origin_low:  vec4<f32>,
    plane_x:           vec4<f32>,
    plane_y:           vec4<f32>,
}
@group(1) @binding(0) var<uniform> h: HatchUniforms;

struct Boundary {
    verts: array<vec4<f32>, 1024>,
}
@group(1) @binding(1) var<uniform> b: Boundary;

// One line family (48 bytes, matches LineFamilyGpu in wipeout_gpu.rs).
struct LineFamily {
    cos_a:      f32,   // base cos(angle)
    sin_a:      f32,   // base sin(angle)
    x0:         f32,   // family origin x
    y0:         f32,   // family origin y
    dx:         f32,   // step vector x
    dy:         f32,   // step vector y
    perp_step:  f32,   // -dx*sin_a + dy*cos_a  (perpendicular spacing)
    along_step: f32,   //  dx*cos_a + dy*sin_a  (phase shift per step)
    line_width: f32,   // half-width of each line (|perp_step| × 0.08)
    period:     f32,   // sum(|dashes|) — 0 means solid
    n_dashes:   u32,   // number of dash entries
    dash_off:   u32,   // start index in dash_values
}

struct FamilyBatch {
    families:    array<LineFamily, 16>,
    dash_values: array<vec4<f32>, 32>,  // 128 f32s packed as 32×vec4 (uniform stride=16)
    n_families:  u32,
    _pad0: u32, _pad1: u32, _pad2: u32,
}
@group(1) @binding(2) var<uniform> f: FamilyBatch;

// ── Vertex shader ──────────────────────────────────────────────────────────

struct VIn  {
    @location(0) pos: vec3<f32>,
    @location(1) translation: vec3<f32>,
    @location(2) translation_low: vec3<f32>,
    @location(3) draw_depth: f32,
}
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)       xz:   vec2<f32>,
}

@vertex fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let local_world = h.plane_x.xyz * v.pos.x + h.plane_y.xyz * v.pos.y;
    let hi = h.plane_origin_high.xyz + v.translation - u.eye_high;
    let lo = local_world + h.plane_origin_low.xyz + v.translation_low - u.eye_low;
    o.clip = u.view_rot * vec4<f32>(hi + lo, 1.0);
    // Draw-order bias (see wire.wgsl): the mask erases what draws BELOW its
    // entity and loses to what draws above — including its own frame wire,
    // which rides a +half-rank override.
    o.clip.z = o.clip.z - (h.draw_depth + v.draw_depth) * 0.001 * o.clip.w;
    o.xz   = vec2<f32>(v.pos.x, v.pos.y);
    return o;
}

// ── Point-in-polygon (ray casting) ────────────────────────────────────────

fn valid_vertex(p: vec2<f32>) -> bool {
    // Sub-loop separators are a huge finite sentinel (1e30, see
    // GPU_BOUNDARY_SEP), not NaN: `x == x` NaN detection gets folded to
    // `true` by some drivers (Intel Mesa fast math), which made separators
    // read as real vertices and bleed fills past their boundary (#386, #416).
    return abs(p.x) < 1.0e29 && abs(p.y) < 1.0e29;
}

fn edge_crosses(p: vec2<f32>, a: vec2<f32>, c: vec2<f32>) -> bool {
    if (a.y > p.y) != (c.y > p.y) {
        let x_int = (c.x - a.x) * (p.y - a.y) / (c.y - a.y) + a.x;
        return p.x < x_int;
    }
    return false;
}

fn in_polygon(p: vec2<f32>) -> bool {
    var inside = false;
    let n = h.vcount;
    var prev = vec2<f32>(0.0, 0.0);
    var first = vec2<f32>(0.0, 0.0);
    var have_prev = false;
    for (var i = 0u; i < n; i++) {
        let vi = b.verts[i].xy;
        if !valid_vertex(vi) {
            // Close the sub-loop that just ended (last → first edge). An
            // unclosed boundary — e.g. a SOLID's 4 corners, which are not
            // repeated — otherwise miscounts crossings and the fill bleeds
            // outside the shape. (#140)
            if have_prev && edge_crosses(p, prev, first) {
                inside = !inside;
            }
            have_prev = false;
            continue;
        }
        if have_prev {
            if edge_crosses(p, prev, vi) {
                inside = !inside;
            }
        } else {
            first = vi;
        }
        prev = vi;
        have_prev = true;
    }
    if have_prev && edge_crosses(p, prev, first) {
        inside = !inside;
    }
    return inside;
}

// ── Per-family hatch test ─────────────────────────────────────────────────
//
// Returns true if world point `xz` falls on a hatch line of `fam`.
// `cos_off` / `sin_off` are cos/sin of `h.angle_offset` (precomputed once).
// `scale` is `h.scale`.

// `ddx_xz`/`ddy_xz` are screen-space derivatives of `xz`, taken once in
// fs_main: derivative builtins must run in uniform control flow, and the
// per-family loop's early return makes later iterations non-uniform.
fn check_family(
    xz:      vec2<f32>,
    ddx_xz:  vec2<f32>,
    ddy_xz:  vec2<f32>,
    fam:     LineFamily,
    cos_off: f32,
    sin_off: f32,
    scale:   f32,
) -> bool {
    // Rotate family direction by angle_offset.
    let cos_a = fam.cos_a * cos_off - fam.sin_a * sin_off;
    let sin_a = fam.sin_a * cos_off + fam.cos_a * sin_off;

    // Rotate and scale the family origin.
    let ox = (fam.x0 * cos_off - fam.y0 * sin_off) * scale;
    let oz = (fam.x0 * sin_off + fam.y0 * cos_off) * scale;

    let px = xz.x - ox;
    let pz = xz.y - oz;

    // Perpendicular distance from the nearest parallel line.
    let perp_step = fam.perp_step * scale;
    let line_w    = abs(fam.line_width * scale);

    let perp   = -px * sin_a + pz * cos_a;
    let k      = round(perp / perp_step);
    let dperp  = perp - k * perp_step;
    let d      = abs(dperp);
    // perp is linear in xz (offsets are constant), so its derivatives are the
    // xz derivatives rotated into the family frame.
    let half_px = length(vec2<f32>(
        -ddx_xz.x * sin_a + ddx_xz.y * cos_a,
        -ddy_xz.x * sin_a + ddy_xz.y * cos_a,
    )) * 0.5;

    // World units per screen pixel on each axis — used to light exactly the
    // one pixel that contains a dot's centre (pixel-snapped, so a dot stays a
    // steady single pixel at any pattern angle instead of flickering).
    let wpx = length(vec2<f32>(ddx_xz.x, ddy_xz.x));
    let wpy = length(vec2<f32>(ddx_xz.y, ddy_xz.y));

    // A fragment within ~1px of a line may be a dot; further out is empty fill.
    if d > half_px * 2.0 { return false; }

    // Solid line family — no dash check needed.
    if fam.n_dashes == 0u { return d <= half_px; }

    // Dash pattern test: position along line k.
    let along_step = fam.along_step * scale;
    let period     = fam.period * scale;
    let along      = px * cos_a + pz * sin_a;
    let t          = along - k * along_step;
    let t_mod      = ((t % period) + period) % period;

    var pos = 0.0;
    for (var j = 0u; j < fam.n_dashes; j++) {
        let idx = fam.dash_off + j;
        let sv  = f.dash_values[idx / 4u][idx % 4u] * scale;  // scale dash lengths
        if sv > 0.0 {
            if d <= half_px && t_mod >= pos && t_mod < pos + sv { return true; }
            pos = pos + sv;
        } else if sv < 0.0 {
            pos = pos - sv;
        } else {
            // Dot: signed distance to its lattice centre, rotated back to world
            // and snapped to the pixel grid — the grid rotates with the
            // pattern but the lit pixel stays a steady single pixel.
            let dtv = (t - pos) - round((t - pos) / period) * period;
            let owx = -dtv * cos_a + dperp * sin_a;
            let owy = -dtv * sin_a - dperp * cos_a;
            if abs(owx / wpx) <= 0.5 && abs(owy / wpy) <= 0.5 { return true; }
        }
    }
    return false;
}

// ── Fragment shader ────────────────────────────────────────────────────────

@fragment fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    // Taken here, in uniform control flow — see check_family.
    let ddx_xz = dpdx(v.xz);
    let ddy_xz = dpdy(v.xz);

    // 1. Boundary test.
    if !in_polygon(v.xz) { discard; }

    // 2. Mode dispatch.
    if h.mode == 1u {
        // Solid fill.
        return h.color;
    } else if h.mode == 2u {
        // Linear gradient.
        let proj = v.xz.x * h.grad_cos + v.xz.y * h.grad_sin;
        let t    = clamp((proj - h.grad_min) / h.grad_range, 0.0, 1.0);
        return mix(h.color, h.color2, t);
    }

    // 3. Pattern: LOD substitution — when the densest family's spacing
    //    projects to less than 2 px, individual lines blur into a solid
    //    fill and the per-family loop just wastes ALU. Return solid color
    //    instead. (Phase 3.3 hatch LOD.)
    if u.world_per_pixel > 0.0 && f.n_families > 0u {
        var min_spacing_world: f32 = 1.0e30;
        for (var i = 0u; i < f.n_families; i++) {
            let s = abs(f.families[i].perp_step) * h.scale;
            if s > 0.0 && s < min_spacing_world {
                min_spacing_world = s;
            }
        }
        if min_spacing_world / u.world_per_pixel < 2.0 {
            return h.color;
        }
    }

    // 4. Pattern coordinates are fill-local.
    let cos_off = cos(h.angle_offset);
    let sin_off = sin(h.angle_offset);
    for (var i = 0u; i < f.n_families; i++) {
        if check_family(v.xz, ddx_xz, ddy_xz, f.families[i], cos_off, sin_off, h.scale) {
            return h.color;
        }
    }
    discard;
    // Unreachable: `discard` kills the fragment before this runs, but
    // DX12/FXC reports E_FAIL X3507 ("not all control paths return a
    // value") without an explicit return after every terminal discard.
    return vec4<f32>(0.0);
}
