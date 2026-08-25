// Texture-backed hatch shader. The kernel mesh bounds the fill; this shader
// evaluates only the pattern, solid, or gradient colour.
//
// Data texture layout (row-major, width = h.tex_width, one vec4 per texel):
//   [ fam_off .. fam_off+3*n_fam )  families, 3 texels each (see load_family)
//   [ dash_off .. )                 dash values, 4 per texel (RGBA)

// ── Group 0: frame uniforms (shared) ──────────────────────────────────────

struct Uniforms {
    viewport_size:       vec2<f32>,
    world_per_pixel:     f32,
    lwdisplay_enable:    f32,
    flat_shade:          f32,
    transparency_enable: f32,
    _pad:                vec2<f32>,
    view_rot:            mat4x4<f32>,
    eye_high:            vec3<f32>,
    _pad_eh:             f32,
    eye_low:             vec3<f32>,
    _pad_el:             f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// ── Group 1: per-hatch data ────────────────────────────────────────────────

struct HatchUniforms {
    color:        vec4<f32>,  //  0
    color2:       vec4<f32>,  // 16
    mode:         u32,        // 32: 0=pattern, 1=solid, 2=gradient
    _reserved:    u32,        // 36
    angle_offset: f32,        // 40
    scale:        f32,        // 44
    grad_cos:     f32,        // 48
    grad_sin:     f32,        // 52
    grad_min:     f32,        // 56
    grad_range:   f32,        // 60
    origin:       vec2<f32>,  // 64
    origin_low:   vec2<f32>,  // 72
    n_families:   u32,        // 80
    fam_off:      u32,        // 84: texel offset of the family section
    dash_off:     u32,        // 88: texel offset of the dash section
    tex_width:    u32,        // 92: data texture width in texels
}
@group(1) @binding(0) var<uniform> h: HatchUniforms;

@group(1) @binding(1) var data_tex: texture_2d<f32>;

// One line family, reconstructed from 3 texels.
struct LineFamily {
    cos_a:      f32,
    sin_a:      f32,
    x0:         f32,
    y0:         f32,
    dx:         f32,
    dy:         f32,
    perp_step:  f32,
    along_step: f32,
    line_width: f32,
    period:     f32,
    n_dashes:   u32,
    dash_off:   u32,
}

fn texel(i: u32) -> vec4<f32> {
    let w = h.tex_width;
    return textureLoad(data_tex, vec2<u32>(i % w, i / w), 0);
}

fn load_family(fi: u32) -> LineFamily {
    let base = h.fam_off + fi * 3u;
    let t0 = texel(base);
    let t1 = texel(base + 1u);
    let t2 = texel(base + 2u);
    var f: LineFamily;
    f.cos_a = t0.x; f.sin_a = t0.y; f.x0 = t0.z; f.y0 = t0.w;
    f.dx = t1.x; f.dy = t1.y; f.perp_step = t1.z; f.along_step = t1.w;
    f.line_width = t2.x; f.period = t2.y;
    // Counts stored as exact f32 (small integers, so no denormal/bitcast hazard
    // through the float texture fetch).
    f.n_dashes = u32(t2.z); f.dash_off = u32(t2.w);
    return f;
}

// ── Vertex shader (identical to wipeout.wgsl) ──────────────────────────────

struct VIn  {
    @location(0) pos: vec3<f32>,
    @location(1) translation: vec2<f32>,
    @location(2) translation_low: vec2<f32>,
    @location(3) draw_depth: f32,
}
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0)       xz:   vec2<f32>,
}

@vertex fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    // Double-single relative-to-eye: anchor high cancels eye_high (Sterbenz);
    // boundary-local v.pos + anchor low + (−eye_low) carry the residual.
    let hi = vec3<f32>(h.origin.x + v.translation.x - u.eye_high.x,
                       h.origin.y + v.translation.y - u.eye_high.y,
                       -u.eye_high.z);
    let lo = vec3<f32>(v.pos.x + h.origin_low.x + v.translation_low.x - u.eye_low.x,
                       v.pos.y + h.origin_low.y + v.translation_low.y - u.eye_low.y,
                       v.pos.z - u.eye_low.z);
    o.clip = u.view_rot * vec4<f32>(hi + lo, 1.0);
    o.clip.z = o.clip.z - v.draw_depth * 0.001 * o.clip.w;
    o.xz   = vec2<f32>(v.pos.x, v.pos.y);
    return o;
}

// ── Per-family hatch test (identical math to wipeout.wgsl) ─────────────────

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
    let width_px = select(1.0, max(fam.line_width, 1.0), u.lwdisplay_enable > 0.5);
    let half_line = half_px * width_px;

    let wpx = length(vec2<f32>(ddx_xz.x, ddy_xz.x));
    let wpy = length(vec2<f32>(ddx_xz.y, ddy_xz.y));

    if d > max(half_line, half_px * 2.0) { return false; }

    if fam.n_dashes == 0u { return d <= half_line; }

    let along_step = fam.along_step * scale;
    let period     = fam.period * scale;
    let along      = px * cos_a + pz * sin_a;
    let t          = along - k * along_step;
    let t_mod      = ((t % period) + period) % period;

    var pos = 0.0;
    for (var j = 0u; j < fam.n_dashes; j++) {
        let idx = fam.dash_off + j;
        let sv  = texel(h.dash_off + idx / 4u)[idx % 4u] * scale;
        if sv > 0.0 {
            if d <= half_line && t_mod >= pos && t_mod < pos + sv { return true; }
            pos = pos + sv;
        } else if sv < 0.0 {
            pos = pos - sv;
        } else {
            let dtv = (t - pos) - round((t - pos) / period) * period;
            let owx = -dtv * cos_a + dperp * sin_a;
            let owy = -dtv * sin_a - dperp * cos_a;
            let dot_half = width_px * 0.5;
            if abs(owx / wpx) <= dot_half && abs(owy / wpy) <= dot_half { return true; }
        }
    }
    return false;
}

// ── Fragment shader ────────────────────────────────────────────────────────

@fragment fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    // Taken here, in uniform control flow — see check_family.
    let ddx_xz = dpdx(v.xz);
    let ddy_xz = dpdy(v.xz);

    let base_mode = h.mode & 0xFFu;
    let gk = (h.mode >> 8u) & 15u;
    let ginv = ((h.mode >> 8u) & 16u) != 0u;
    if base_mode == 1u {
        return h.color;
    } else if base_mode == 2u {
        let proj = v.xz.x * h.grad_cos + v.xz.y * h.grad_sin;
        var t    = clamp((proj - h.grad_min) / h.grad_range, 0.0, 1.0);
        // Shape profile: cylinder mirrors around the middle, curved eases in.
        if gk == 1u {
            t = 1.0 - abs(2.0 * t - 1.0);
        } else if gk == 4u {
            t = t * t;
        }
        if ginv {
            t = 1.0 - t;
        }
        return mix(h.color, h.color2, t);
    } else if base_mode == 3u {
        // Radial gradient: centre is (grad_cos, grad_sin), radius is grad_range.
        let d = length(v.xz - vec2<f32>(h.grad_cos, h.grad_sin));
        var t = clamp(d / h.grad_range, 0.0, 1.0);
        if gk == 3u {
            t = sqrt(t);
        }
        if ginv {
            t = 1.0 - t;
        }
        // Radial stops run outside-in (colour 2 at the centre) — see hatch.wgsl.
        return mix(h.color2, h.color, t);
    }

    // Keep every family visible until all family spacings project below 2 px,
    // then substitute one solid fill. A single dense family must neither hide
    // itself nor turn a complex hatch solid.
    var all_families_subpixel = h.n_families > 0u && u.world_per_pixel > 0.0;
    if all_families_subpixel {
        for (var i = 0u; i < h.n_families; i++) {
            let spacing_world = abs(load_family(i).perp_step) * h.scale;
            if spacing_world <= 0.0 {
                all_families_subpixel = false;
            } else if spacing_world / u.world_per_pixel >= 2.0 {
                all_families_subpixel = false;
            }
        }
    }
    if all_families_subpixel {
        return h.color;
    }

    let cos_off = cos(h.angle_offset);
    let sin_off = sin(h.angle_offset);
    for (var i = 0u; i < h.n_families; i++) {
        let fam = load_family(i);
        if check_family(v.xz, ddx_xz, ddy_xz, fam, cos_off, sin_off, h.scale) {
            return h.color;
        }
    }
    discard;
    // Unreachable, but FXC/DX12 requires a return after every terminal discard.
    return vec4<f32>(0.0);
}
