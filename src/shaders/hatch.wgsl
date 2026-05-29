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
    view_proj:       mat4x4<f32>,
    camera_pos:      vec4<f32>,
    viewport_size:   vec2<f32>,
    world_per_pixel: f32,
    _pad:            f32,
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
    origin:       vec2<f32>,  // 64: hatch-local pattern origin (boundary
    //                        //     centre). Pattern math runs against
    //                        //     `xz - origin` so the f32 modulo doesn't
    //                        //     catastrophically cancel when world coords
    //                        //     are large and pattern spacing is small.
    _pad:         vec2<f32>,  // 72: 16-byte alignment
}
@group(1) @binding(0) var<uniform> h: HatchUniforms;

struct Boundary {
    verts: array<vec4<f32>, 1024>,
}
@group(1) @binding(1) var<uniform> b: Boundary;

// One line family (48 bytes, matches LineFamilyGpu in hatch_gpu.rs).
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

struct VIn  { @location(0) pos: vec3<f32> }
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)       xz:   vec2<f32>,
}

@vertex fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    // v.pos comes in pre-shifted to hatch-local space (CPU subtracted
    // h.origin from the quad). Add origin back inside the view_proj
    // multiply to land at the correct world-space clip position. The
    // f32+f32 addition can lose sub-mm precision at very far hatches
    // but the result feeds view_proj for screen NDC where sub-pixel
    // jitter is invisible. The local-space `xz` we export to the
    // fragment shader stays at full f32 precision so the in_polygon /
    // pattern math doesn't catastrophically cancel.
    let world = vec3<f32>(v.pos.x + h.origin.x, v.pos.y + h.origin.y, v.pos.z);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    o.xz   = vec2<f32>(v.pos.x, v.pos.y);
    return o;
}

// ── Point-in-polygon (ray casting) ────────────────────────────────────────

fn valid_vertex(p: vec2<f32>) -> bool {
    return p.x == p.x && p.y == p.y;
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
    var have_prev = false;
    for (var i = 0u; i < n; i++) {
        let vi = b.verts[i].xy;
        if !valid_vertex(vi) {
            have_prev = false;
            continue;
        }
        if have_prev && edge_crosses(p, prev, vi) {
            inside = !inside;
        }
        prev = vi;
        have_prev = true;
    }
    return inside;
}

// ── Per-family hatch test ─────────────────────────────────────────────────
//
// Returns true if world point `xz` falls on a hatch line of `fam`.
// `cos_off` / `sin_off` are cos/sin of `h.angle_offset` (precomputed once).
// `scale` is `h.scale`.

fn check_family(
    xz:      vec2<f32>,
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
    let d      = abs(perp - k * perp_step);
    let half_px = length(vec2<f32>(dpdx(perp), dpdy(perp))) * 0.5;

    if d > half_px { return false; }

    // Solid line family — no dash check needed.
    if fam.n_dashes == 0u { return true; }

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
            if t_mod >= pos && t_mod < pos + sv { return true; }
            pos = pos + sv;
        } else {
            pos = pos - sv;
        }
    }
    return false;
}

// ── Fragment shader ────────────────────────────────────────────────────────

@fragment fn fs_main(v: VOut) -> @location(0) vec4<f32> {
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

    // 4. Pattern: v.xz is already in hatch-local space (CPU pre-shifted
    //    quad + BoundaryData by h.origin). The PAT family origin
    //    (typically (0,0)) is treated as relative to h.origin, so
    //    pattern phase is per-hatch rather than world-aligned — that
    //    trade-off is the price of f32 precision at large drawing
    //    extents.
    let cos_off = cos(h.angle_offset);
    let sin_off = sin(h.angle_offset);
    for (var i = 0u; i < f.n_families; i++) {
        if check_family(v.xz, f.families[i], cos_off, sin_off, h.scale) {
            return h.color;
        }
    }
    discard;
    // Unreachable: `discard` kills the fragment before this runs, but
    // DX12/FXC reports E_FAIL X3507 ("not all control paths return a
    // value") without an explicit return after every terminal discard.
    return vec4<f32>(0.0);
}
