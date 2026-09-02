// Batched hatch shader. Kernel mesh vertices carry their source instance.
//
// Layout — matches `hatch_gpu/storage.rs`:
//   group 1 binding 0  InstanceBuffer  HatchInstance[]
//   group 1 binding 1  FamilyBuffer    LineFamilyGpu[]
//   group 1 binding 2  DashBuffer      f32[]
//   group 1 binding 3  Visibility      u32[]

// ── Group 0: shared frame uniforms (matches hatch.wgsl) ──────────────────

struct Uniforms {
    viewport_size:       vec2<f32>,
    world_per_pixel:     f32,
    lwdisplay_enable:    f32,
    flat_shade:          f32,
    transparency_enable: f32,
    _linetype_scale:     f32,
    lineweight_scale:    f32,
    // Relative-to-eye (double-single): see wire.wgsl.
    view_rot:            mat4x4<f32>,
    eye_high:            vec3<f32>,
    _pad_eh:             f32,
    eye_low:             vec3<f32>,
    _pad_el:             f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Group 1: batched hatch storage ───────────────────────────────────────

struct HatchInstance {
    color:           vec4<f32>,
    color2:          vec4<f32>,
    aabb:            vec4<f32>,   // (xmin, ymin, xmax, ymax) — local space
    world_origin:    vec2<f32>,     // anchor high half
    world_origin_low: vec2<f32>,    // anchor low residual (double-single)
    angle_offset:    f32,
    scale:           f32,
    grad_cos:        f32,
    grad_sin:        f32,
    grad_min:        f32,
    grad_range:      f32,
    mode:            u32,         // 0=pattern, 1=solid, 2=gradient
    visible:         u32,         // 0 = skip (CPU writes via compute_hatch_lod)
    family_offset:   u32,
    family_count:    u32,
    draw_depth:      f32,          // signed (-1,1) draw-order bias; 0 = neutral
    grad_kind:       u32,         // shape (0=linear,1=cyl,2=sph,3=hemi,4=curved), bit4=invert
}

// Draw-order depth bias (see wire.wgsl). Higher draw_depth → smaller z →
// drawn on top, ordering this fill against other entity types.
const DRAW_ORDER_BIAS: f32 = 0.001;
const MODEL_LINEWEIGHT_BOOST: f32 = 2.0;
const MODEL_LINEWEIGHT_MAX_PX: f32 = 10.0;

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
@group(1) @binding(1) var<storage, read> families:   array<LineFamily>;
@group(1) @binding(2) var<storage, read> dashes:     array<f32>;
// Per-instance visibility (Phase 4-B sub-pixel + frustum skip).
// CPU writes `1` to draw / `0` to skip every frame; vertex shader
// emits an out-of-NDC clip position for 0-instances so the GPU
// rasterizer culls the primitive before any fragment runs.
@group(1) @binding(3) var<storage, read> visibility: array<u32>;

// ── Vertex shader ────────────────────────────────────────────────────────

struct VIn {
    @location(0) local_xy:       vec2<f32>,
    @location(1) instance_index: u32,
    @location(2) translation: vec2<f32>,
    @location(3) translation_low: vec2<f32>,
    @location(4) draw_depth: f32,
    @location(5) visible: u32,
}

struct VOut {
    @builtin(position) clip:           vec4<f32>,
    @location(0)       xz:             vec2<f32>,
    @location(1) @interpolate(flat) instance_index: u32,
}

@vertex fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let inst = instances[v.instance_index];

    // Per-frame visibility (CPU-driven sub-pixel + frustum skip).
    // 0 → emit a clip position whose x/y exceed |w| so the GPU
    // frustum-culls the primitive and no fragment runs. (WGSL
    // forbids literal NaN so this out-of-NDC trick replaces the
    // usual NaN-degenerate-triangle.)
    if visibility[v.instance_index] == 0u || v.visible == 0u {
        o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
        o.xz = vec2<f32>(0.0, 0.0);
        o.instance_index = v.instance_index;
        return o;
    }

    let local = v.local_xy;
    // Double-single relative-to-eye: the anchor high half cancels exactly
    // against eye_high (Sterbenz); local + anchor low + (−eye_low) carry the
    // residual. `local` is small (boundary-relative), so adding it in the low
    // term keeps full precision at UTM-scale anchors.
    let hi = vec3<f32>(inst.world_origin.x + v.translation.x - u.eye_high.x,
                       inst.world_origin.y + v.translation.y - u.eye_high.y,
                       -u.eye_high.z);
    let lo = vec3<f32>(local.x + inst.world_origin_low.x + v.translation_low.x - u.eye_low.x,
                       local.y + inst.world_origin_low.y + v.translation_low.y - u.eye_low.y,
                       -u.eye_low.z);
    o.clip = u.view_rot * vec4<f32>(hi + lo, 1.0);
    o.clip.z = o.clip.z
        - (inst.draw_depth + v.draw_depth) * DRAW_ORDER_BIAS * o.clip.w;
    o.xz = local;
    o.instance_index = v.instance_index;
    return o;
}

// ── Per-family hatch test (same math as hatch.wgsl, dashes from
// global DashBuffer instead of per-hatch FamilyBatch) ────────────────────

// `ddx_xz`/`ddy_xz` are screen-space derivatives of `xz`, taken once in
// fs_main: derivative builtins must run in uniform control flow, and the
// per-family loop's early return makes later iterations non-uniform.
fn display_lineweight(width: f32) -> f32 {
    if u.lineweight_scale < 0.0 {
        let base = select(
            min(width * MODEL_LINEWEIGHT_BOOST, MODEL_LINEWEIGHT_MAX_PX),
            1.0,
            width <= 1.0,
        );
        return max(base * -u.lineweight_scale, 1.0);
    }
    return max(width * u.lineweight_scale, 1.0);
}

fn check_family(
    xz:      vec2<f32>,
    ddx_xz:  vec2<f32>,
    ddy_xz:  vec2<f32>,
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
    let width_px = select(1.0, display_lineweight(fam.line_width), u.lwdisplay_enable > 0.5);
    let half_line = half_px * width_px;

    // World units per screen pixel on each axis — used to light exactly the
    // one pixel that contains a dot's centre (pixel-snapped, so the dot stays
    // a steady single pixel at any pattern angle instead of flickering).
    let wpx = length(vec2<f32>(ddx_xz.x, ddy_xz.x));
    let wpy = length(vec2<f32>(ddx_xz.y, ddy_xz.y));

    // A fragment within ~1px of a line may be a dot; everything further out is
    // empty fill. (A dot's pixel sits on a line, so its perp offset is < 1px.)
    if d > max(half_line, half_px * 2.0) { return false; }
    if fam.n_dashes == 0u { return d <= half_line; }

    let along_step = fam.along_step * scale;
    let period     = fam.period * scale;
    let along      = px * cos_a + pz * sin_a;
    let t          = along - k * along_step;
    let t_mod      = ((t % period) + period) % period;

    var pos = 0.0;
    for (var j = 0u; j < fam.n_dashes; j++) {
        let sv = dashes[fam.dash_offset + j] * scale;
        if sv > 0.0 {
            if d <= half_line && t_mod >= pos && t_mod < pos + sv { return true; }
            pos = pos + sv;
        } else if sv < 0.0 {
            pos = pos - sv;
        } else {
            // Dot: signed distance to its lattice centre (along the line and
            // across lines), rotated back to world, then snapped to the pixel
            // grid. The dot grid rotates with the pattern; the lit pixel does
            // not, so it never thins/flickers.
            let dtv = (t - pos) - round((t - pos) / period) * period;
            let owx = -dtv * cos_a + dperp * sin_a;
            let owy = -dtv * sin_a - dperp * cos_a;
            let dot_half = width_px * 0.5;
            if abs(owx / wpx) <= dot_half && abs(owy / wpy) <= dot_half { return true; }
        }
    }
    return false;
}

// ── Fragment shader ──────────────────────────────────────────────────────

@fragment fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    // Taken here, in uniform control flow — see check_family.
    let ddx_xz = dpdx(v.xz);
    let ddy_xz = dpdy(v.xz);

    let inst = instances[v.instance_index];

    // Mode dispatch.
    if inst.mode == 1u {
        return inst.color;
    } else if inst.mode == 2u {
        let proj = v.xz.x * inst.grad_cos + v.xz.y * inst.grad_sin;
        var t = clamp((proj - inst.grad_min) / inst.grad_range, 0.0, 1.0);
        // Shape profile: cylinder mirrors around the middle, curved eases in.
        let k = inst.grad_kind & 15u;
        if k == 1u {
            t = 1.0 - abs(2.0 * t - 1.0);
        } else if k == 4u {
            t = t * t;
        }
        if (inst.grad_kind & 16u) != 0u {
            t = 1.0 - t;
        }
        return mix(inst.color, inst.color2, t);
    } else if inst.mode == 3u {
        // Radial gradient: centre is (grad_cos, grad_sin), radius is grad_range.
        let d = length(v.xz - vec2<f32>(inst.grad_cos, inst.grad_sin));
        var t = clamp(d / inst.grad_range, 0.0, 1.0);
        // Hemispherical shades faster near the centre (dome profile).
        if (inst.grad_kind & 15u) == 3u {
            t = sqrt(t);
        }
        if (inst.grad_kind & 16u) != 0u {
            t = 1.0 - t;
        }
        // Radial stops run OUTSIDE-IN: colour 1 at the rim, colour 2 at the
        // centre; the INV bit above swaps them back.
        return mix(inst.color2, inst.color, t);
    }

    // Pattern LOD. Keep every family visible until all family spacings
    //    project below 2 px, then substitute one solid fill. A single dense
    //    family must neither hide itself nor turn a complex hatch solid.
    var all_families_subpixel = inst.family_count > 0u && u.world_per_pixel > 0.0;
    if all_families_subpixel {
        for (var i = 0u; i < inst.family_count; i++) {
            let spacing_world =
                abs(families[inst.family_offset + i].perp_step) * inst.scale;
            if spacing_world <= 0.0 {
                all_families_subpixel = false;
            } else if spacing_world / u.world_per_pixel >= 2.0 {
                all_families_subpixel = false;
            }
        }
    }
    if all_families_subpixel {
        return inst.color;
    }

    // Pattern evaluation.
    let cos_off = cos(inst.angle_offset);
    let sin_off = sin(inst.angle_offset);
    for (var i = 0u; i < inst.family_count; i++) {
        let fam = families[inst.family_offset + i];
        if check_family(v.xz, ddx_xz, ddy_xz, fam, cos_off, sin_off, inst.scale) {
            return inst.color;
        }
    }
    discard;
    // Unreachable: `discard` kills the fragment before this runs, but
    // DX12/FXC reports E_FAIL X3507 ("not all control paths return a
    // value") without an explicit return after every terminal discard.
    return vec4<f32>(0.0);
}
