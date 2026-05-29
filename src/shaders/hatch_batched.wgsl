// Phase 4-B — batched hatch shader. All hatches in one draw call;
// per-instance data fetched from storage buffers indexed by the
// `instance_index` vertex attribute (passed from the per-vertex
// (corner, instance_index) stream so we don't depend on
// @builtin(instance_index) edge cases across backends).
//
// Layout — matches `hatch_batched_gpu.rs`:
//   group 1 binding 0  InstanceBuffer  HatchInstance[]   (112 B / inst)
//   group 1 binding 1  BoundaryBuffer  vec4<f32>[]       (xy in .xy)
//   group 1 binding 2  FamilyBuffer    LineFamilyGpu[]   (48 B / fam)
//   group 1 binding 3  DashBuffer      f32[]
//
// Vertex shader emits a per-instance AABB quad (two triangles). A
// `visible == 0` instance gets a NaN clip position so the fragment
// shader never runs for it — that's the GPU-side cull (Phase 4-B
// equivalent of `compute_hatch_lod` writing `hatch_skip_flags`).

// ── Group 0: shared frame uniforms (matches hatch.wgsl) ──────────────────

struct Uniforms {
    view_proj:       mat4x4<f32>,
    camera_pos:      vec4<f32>,
    viewport_size:   vec2<f32>,
    world_per_pixel: f32,
    _pad:            f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Group 1: batched hatch storage ───────────────────────────────────────

struct HatchInstance {
    color:           vec4<f32>,
    color2:          vec4<f32>,
    aabb:            vec4<f32>,   // (xmin, ymin, xmax, ymax) — local space
    world_origin:    vec2<f32>,
    angle_offset:    f32,
    scale:           f32,
    grad_cos:        f32,
    grad_sin:        f32,
    grad_min:        f32,
    grad_range:      f32,
    mode:            u32,         // 0=pattern, 1=solid, 2=gradient
    visible:         u32,         // 0 = skip (CPU writes via compute_hatch_lod)
    boundary_offset: u32,
    boundary_count:  u32,
    family_offset:   u32,
    family_count:    u32,
    _pad0:           vec2<u32>,
}

struct LineFamily {
    cos_a:       f32,
    sin_a:       f32,
    x0:          f32,
    y0:          f32,
    dx:          f32,
    dy:          f32,
    perp_step:   f32,
    along_step:  f32,
    line_width:  f32,
    period:      f32,
    n_dashes:    u32,
    dash_offset: u32,
}

@group(1) @binding(0) var<storage, read> instances:  array<HatchInstance>;
@group(1) @binding(1) var<storage, read> boundary:   array<vec4<f32>>;
@group(1) @binding(2) var<storage, read> families:   array<LineFamily>;
@group(1) @binding(3) var<storage, read> dashes:     array<f32>;
// Per-instance visibility (Phase 4-B sub-pixel + frustum skip).
// CPU writes `1` to draw / `0` to skip every frame; vertex shader
// emits an out-of-NDC clip position for 0-instances so the GPU
// rasterizer culls the primitive before any fragment runs.
@group(1) @binding(4) var<storage, read> visibility: array<u32>;

// ── Vertex shader ────────────────────────────────────────────────────────

struct VIn {
    @location(0) corner:         u32,
    @location(1) instance_index: u32,
}

struct VOut {
    @builtin(position) clip:           vec4<f32>,
    @location(0)       xz:             vec2<f32>,
    @location(1) @interpolate(flat) instance_index: u32,
}

fn corner_xy(c: u32, aabb: vec4<f32>) -> vec2<f32> {
    // Two-triangle quad covering the AABB:
    //   0 BL, 1 BR, 2 TL, 3 BR, 4 TR, 5 TL
    let xmin = aabb.x; let ymin = aabb.y;
    let xmax = aabb.z; let ymax = aabb.w;
    switch c {
        case 0u: { return vec2<f32>(xmin, ymin); }
        case 1u: { return vec2<f32>(xmax, ymin); }
        case 2u: { return vec2<f32>(xmin, ymax); }
        case 3u: { return vec2<f32>(xmax, ymin); }
        case 4u: { return vec2<f32>(xmax, ymax); }
        default: { return vec2<f32>(xmin, ymax); }
    }
}

@vertex fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let inst = instances[v.instance_index];

    // Per-frame visibility (CPU-driven sub-pixel + frustum skip).
    // 0 → emit a clip position whose x/y exceed |w| so the GPU
    // frustum-culls the primitive and no fragment runs. (WGSL
    // forbids literal NaN so this out-of-NDC trick replaces the
    // usual NaN-degenerate-triangle.)
    if visibility[v.instance_index] == 0u {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        o.xz = vec2<f32>(0.0, 0.0);
        o.instance_index = v.instance_index;
        return o;
    }

    let local = corner_xy(v.corner, inst.aabb);
    let world = vec3<f32>(local.x + inst.world_origin.x,
                          local.y + inst.world_origin.y,
                          0.0);
    o.clip = u.view_proj * vec4<f32>(world, 1.0);
    o.xz = local;
    o.instance_index = v.instance_index;
    return o;
}

// ── Point-in-polygon (ray casting) over a sub-range of BoundaryBuffer ────

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

fn in_polygon(p: vec2<f32>, offset: u32, count: u32) -> bool {
    var inside = false;
    var prev = vec2<f32>(0.0, 0.0);
    var have_prev = false;
    for (var i = 0u; i < count; i++) {
        let vi = boundary[offset + i].xy;
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

// ── Per-family hatch test (same math as hatch.wgsl, dashes from
// global DashBuffer instead of per-hatch FamilyBatch) ────────────────────

fn check_family(
    xz:      vec2<f32>,
    fam:     LineFamily,
    cos_off: f32,
    sin_off: f32,
    scale:   f32,
) -> bool {
    let cos_a = fam.cos_a * cos_off - fam.sin_a * sin_off;
    let sin_a = fam.sin_a * cos_off + fam.cos_a * sin_off;

    let ox = (fam.x0 * cos_off - fam.y0 * sin_off) * scale;
    let oz = (fam.x0 * sin_off + fam.y0 * cos_off) * scale;

    let px = xz.x - ox;
    let pz = xz.y - oz;

    let perp_step = fam.perp_step * scale;
    let line_w    = abs(fam.line_width * scale);

    let perp   = -px * sin_a + pz * cos_a;
    let k      = round(perp / perp_step);
    let d      = abs(perp - k * perp_step);
    let half_px = length(vec2<f32>(dpdx(perp), dpdy(perp))) * 0.5;

    if d > half_px { return false; }
    if fam.n_dashes == 0u { return true; }

    let along_step = fam.along_step * scale;
    let period     = fam.period * scale;
    let along      = px * cos_a + pz * sin_a;
    let t          = along - k * along_step;
    let t_mod      = ((t % period) + period) % period;

    var pos = 0.0;
    for (var j = 0u; j < fam.n_dashes; j++) {
        let sv = dashes[fam.dash_offset + j] * scale;
        if sv > 0.0 {
            if t_mod >= pos && t_mod < pos + sv { return true; }
            pos = pos + sv;
        } else {
            pos = pos - sv;
        }
    }
    return false;
}

// ── Fragment shader ──────────────────────────────────────────────────────

@fragment fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    let inst = instances[v.instance_index];

    // 1. Boundary test.
    if !in_polygon(v.xz, inst.boundary_offset, inst.boundary_count) {
        discard;
    }

    // 2. Mode dispatch.
    if inst.mode == 1u {
        return inst.color;
    } else if inst.mode == 2u {
        let proj = v.xz.x * inst.grad_cos + v.xz.y * inst.grad_sin;
        let t = clamp((proj - inst.grad_min) / inst.grad_range, 0.0, 1.0);
        return mix(inst.color, inst.color2, t);
    }

    // 3. Pattern LOD: when the densest family's spacing projects below
    //    2 px, lines blur into a solid fill — return color instead of
    //    iterating every family (mirrors Phase 3.3 LOD in hatch.wgsl).
    if u.world_per_pixel > 0.0 && inst.family_count > 0u {
        var min_spacing_world: f32 = 1.0e30;
        for (var i = 0u; i < inst.family_count; i++) {
            let s = abs(families[inst.family_offset + i].perp_step) * inst.scale;
            if s > 0.0 && s < min_spacing_world {
                min_spacing_world = s;
            }
        }
        if min_spacing_world / u.world_per_pixel < 2.0 {
            return inst.color;
        }
    }

    // 4. Pattern evaluation.
    let cos_off = cos(inst.angle_offset);
    let sin_off = sin(inst.angle_offset);
    for (var i = 0u; i < inst.family_count; i++) {
        if check_family(v.xz, families[inst.family_offset + i], cos_off, sin_off, inst.scale) {
            return inst.color;
        }
    }
    discard;
    // Unreachable: `discard` kills the fragment before this runs, but
    // DX12/FXC reports E_FAIL X3507 ("not all control paths return a
    // value") without an explicit return after every terminal discard.
    return vec4<f32>(0.0);
}
