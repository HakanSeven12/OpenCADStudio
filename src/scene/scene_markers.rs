//! Synthesized nonprint helper glyphs that are *not* CAD entities.
//!
//! Some drawing decorations are derived from document objects/tables rather
//! than from an entity in model space, so they never ride the per-entity
//! resident tessellation. Today that is the geographic-location "daisy" marker
//! (from the `GEODATA` object). These are appended to the Model-space resident
//! wire set by [`Scene::append_scene_markers`].
//!
//! Coordinates are absolute f64 (drawings can sit at UTM-scale origins), so the
//! wires are built with [`WireModel::solid_f64`], which fills the double-single
//! residual buffer and keeps the glyph precise and jitter-free.

use std::f64::consts::TAU;

use codec::objects::ObjectType;

use crate::scene::model::wire_model::WireModel;
use crate::scene::Scene;

/// World-space radius of a synthesized marker glyph.
const MARKER_R: f64 = 3.0;

const NAN: [f64; 3] = [f64::NAN, f64::NAN, f64::NAN];

fn push_ring(out: &mut Vec<[f64; 3]>, c: [f64; 3], r: f64, n: usize) {
    if !out.is_empty() {
        out.push(NAN);
    }
    for i in 0..=n {
        let a = i as f64 * TAU / n as f64;
        out.push([c[0] + a.cos() * r, c[1] + a.sin() * r, c[2]]);
    }
}

fn push_seg(out: &mut Vec<[f64; 3]>, a: [f64; 3], b: [f64; 3]) {
    if !out.is_empty() {
        out.push(NAN);
    }
    out.push(a);
    out.push(b);
}

// ── vector helpers for oriented glyphs ─────────────────────────────────────

fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn v_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn v_norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

/// Two unit axes spanning the plane perpendicular to `a`.
fn perp_basis(a: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let up = if a[2].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let u = v_norm(v_cross(up, a));
    let v = v_cross(a, u);
    (u, v)
}

/// Camera display glyph at `cam_pos`, aimed along `fwd` (unit, toward the
/// subject): an oriented body box with a lens cone poking out the front.
fn camera_glyph(cam_pos: [f64; 3], fwd: [f64; 3], r: f64) -> Vec<[f64; 3]> {
    let (u, v) = perp_basis(fwd);
    // Point in the camera frame: `along` runs on the aim axis (front is +).
    let f = |along: f64, su: f64, sv: f64| -> [f64; 3] {
        [
            cam_pos[0] + fwd[0] * along + u[0] * su + v[0] * sv,
            cam_pos[1] + fwd[1] * along + u[1] * su + v[1] * sv,
            cam_pos[2] + fwd[2] * along + u[2] * su + v[2] * sv,
        ]
    };
    let mut out: Vec<[f64; 3]> = Vec::new();
    // Body box: behind the lens plane (along −1.3r..−0.3r).
    let (bw, bh) = (0.6 * r, 0.45 * r);
    let (zb, zf) = (-1.3 * r, -0.3 * r);
    let corner = |along: f64, sx: f64, sy: f64| f(along, sx * bw, sy * bh);
    let back = [
        corner(zb, -1.0, -1.0),
        corner(zb, 1.0, -1.0),
        corner(zb, 1.0, 1.0),
        corner(zb, -1.0, 1.0),
    ];
    let front = [
        corner(zf, -1.0, -1.0),
        corner(zf, 1.0, -1.0),
        corner(zf, 1.0, 1.0),
        corner(zf, -1.0, 1.0),
    ];
    for k in 0..4 {
        let n = (k + 1) % 4;
        push_seg(&mut out, back[k], back[n]);
        push_seg(&mut out, front[k], front[n]);
        push_seg(&mut out, back[k], front[k]);
    }
    // Lens cone: apex at the body front, rim ring in front of the camera.
    let apex = f(zf, 0.0, 0.0);
    let rim_along = 0.5 * r;
    let rim_r = 0.35 * r;
    let n = 16;
    if !out.is_empty() {
        out.push(NAN);
    }
    for i in 0..=n {
        let a = i as f64 * TAU / n as f64;
        out.push(f(rim_along, a.cos() * rim_r, a.sin() * rim_r));
    }
    for k in 0..4 {
        let a = k as f64 * TAU / 4.0;
        push_seg(
            &mut out,
            apex,
            f(rim_along, a.cos() * rim_r, a.sin() * rim_r),
        );
    }
    out
}

/// Build the geographic-location "daisy": concentric rings plus radial petals,
/// with a longer spoke marking the drawing's north direction.
fn geo_daisy(center: [f64; 3], north: [f64; 2], r: f64) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    push_ring(&mut out, center, r, 24);
    push_ring(&mut out, center, r * 0.45, 16);
    // 12 radial petals from the inner ring to the rim.
    for k in 0..12 {
        let a = k as f64 * TAU / 12.0;
        let (ca, sa) = (a.cos(), a.sin());
        push_seg(
            &mut out,
            [
                center[0] + ca * r * 0.45,
                center[1] + sa * r * 0.45,
                center[2],
            ],
            [center[0] + ca * r, center[1] + sa * r, center[2]],
        );
    }
    // North spoke — normalized north direction, defaulting to +Y.
    let nlen = (north[0] * north[0] + north[1] * north[1]).sqrt();
    let (nx, ny) = if nlen < 1e-9 {
        (0.0, 1.0)
    } else {
        (north[0] / nlen, north[1] / nlen)
    };
    push_seg(
        &mut out,
        center,
        [
            center[0] + nx * r * 1.6,
            center[1] + ny * r * 1.6,
            center[2],
        ],
    );
    out
}

impl Scene {
    /// Append synthesized nonprint markers to a freshly built Model-space
    /// resident wire set. No-op for any block other than model space.
    pub(super) fn append_scene_markers(&self, wires: &mut Vec<WireModel>, _bg: [f32; 4]) {
        // Geographic-location daisy (one per GEODATA object; normally one).
        for obj in self.document.objects.values() {
            if let ObjectType::GeoData(g) = obj {
                let c = [g.design_point.x, g.design_point.y, g.design_point.z];
                let north = [g.north_direction.x, g.north_direction.y];
                let pts = geo_daisy(c, north, MARKER_R);
                // A muted geo-marker colour (nonprint helper); the reader keeps
                // no display colour for GEODATA, so use a fixed grey-cyan.
                let w = WireModel::solid_f64(
                    format!("geomarker-{}", u64::from(g.handle)),
                    pts,
                    [0.55, 0.75, 0.85, 1.0],
                    false,
                );
                wires.push(w);
            }
        }

        // Camera glyphs — one per perspective VIEW (created by the CAMERA
        // command; VIEWMODE perspective bit). The camera sits at target +
        // direction and looks back toward the target.
        for view in self.document.views.iter() {
            if !view.perspective {
                continue;
            }
            let target = [view.target.x, view.target.y, view.target.z];
            let dir = [view.direction.x, view.direction.y, view.direction.z];
            let cam_pos = [target[0] + dir[0], target[1] + dir[1], target[2] + dir[2]];
            // Aim from the camera back toward the subject (= −direction).
            let fwd = v_norm(v_sub(target, cam_pos));
            let fwd = if fwd == [0.0, 0.0, 0.0] {
                [0.0, 0.0, -1.0]
            } else {
                fwd
            };
            let pts = camera_glyph(cam_pos, fwd, MARKER_R);
            let w = WireModel::solid_f64(
                format!("camera-{}", u64::from(view.handle)),
                pts,
                [0.60, 0.70, 0.90, 1.0],
                false,
            );
            wires.push(w);
        }
    }
}
