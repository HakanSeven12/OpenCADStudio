//! Surface analysis shown on shaded surfaces and solids (ANALYSISZEBRA, ANALYSISCURVATURE,
//! ANALYSISDRAFT, ANALYSISOPTIONS): each triangle of a shaded mesh coloured by its stripe, its
//! curvature or its draft angle, read off the mesh's positions and normals.

use std::sync::Mutex;

use crate::scene::model::mesh_model::{MeshLodSet, MeshModel};

/// What the analysis shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Zebra,
    Curvature,
    Draft,
}

struct Settings {
    mode: Option<Mode>,
    /// Zebra stripes across the half sphere of normals.
    stripes: f64,
    /// 0 Gaussian, 1 mean, 2 maximum, 3 minimum curvature.
    curvature: u8,
    /// The draft angles, degrees, shown at the low and high colours.
    draft: [f64; 2],
}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    mode: None,
    stripes: 8.0,
    curvature: 0,
    draft: [-3.0, 3.0],
});

fn settings() -> std::sync::MutexGuard<'static, Settings> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The analysis shown, if any.
pub fn active() -> Option<Mode> {
    settings().mode
}

/// Turns `mode` on (and any other off), or off where it is on; whether it is on now.
pub fn toggle(mode: Mode) -> bool {
    let mut s = settings();
    s.mode = if s.mode == Some(mode) { None } else { Some(mode) };
    s.mode.is_some()
}

/// ANALYSISOPTIONS: the values given replace the ones kept.
pub fn set_options(stripes: Option<f64>, curvature: Option<f64>, draft_min: Option<f64>, draft_max: Option<f64>) {
    let mut s = settings();
    if let Some(v) = stripes.filter(|v| v.is_finite() && *v >= 1.0) {
        s.stripes = v.round().min(100.0);
    }
    if let Some(v) = curvature.filter(|v| v.is_finite()) {
        s.curvature = v.round().clamp(0.0, 3.0) as u8;
    }
    if let Some(v) = draft_min.filter(|v| v.is_finite()) {
        s.draft[0] = v.clamp(-90.0, 90.0);
    }
    if let Some(v) = draft_max.filter(|v| v.is_finite()) {
        s.draft[1] = v.clamp(-90.0, 90.0);
    }
    if s.draft[0] > s.draft[1] {
        s.draft.swap(0, 1);
    }
}

/// A colour on the blue → green → red ramp, `t` in -1 ..= 1.
fn ramp(t: f64) -> [f32; 4] {
    let t = t.clamp(-1.0, 1.0) as f32;
    if t < 0.0 {
        [0.0, 1.0 + t, -t, 1.0]
    } else {
        [t, 1.0 - t, 0.0, 1.0]
    }
}

fn position(lod: &MeshModel, k: usize) -> [f64; 3] {
    let v = lod.verts[k];
    let low = lod.verts_low.get(k).copied().unwrap_or([0.0; 3]);
    std::array::from_fn(|c| f64::from(v[c]) + f64::from(low[c]))
}

fn normal(lod: &MeshModel, k: usize) -> [f64; 3] {
    let n = lod.normals.get(k).copied().unwrap_or([0.0, 0.0, 1.0]);
    n.map(f64::from)
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Colours every triangle of `lod` by the analysis shown.
fn colour_lod(lod: &mut MeshModel, mode: Mode, s: &Settings) {
    let triangles = lod.indices.len() / 3;
    let corner = |t: usize, c: usize| lod.indices[3 * t + c] as usize;
    let mut values = Vec::with_capacity(triangles);
    for t in 0..triangles {
        let [a, b, c] = [corner(t, 0), corner(t, 1), corner(t, 2)];
        let n = {
            let sum = [normal(lod, a), normal(lod, b), normal(lod, c)]
                .iter()
                .fold([0.0; 3], |acc, n| [acc[0] + n[0], acc[1] + n[1], acc[2] + n[2]]);
            let len = dot(sum, sum).sqrt().max(1e-300);
            sum.map(|x| x / len)
        };
        let value = match mode {
            // Bands of the normal's lean along a level direction, as parallel light strips
            // mirrored in the surface: a flat face is one band, a curved one stripes.
            Mode::Zebra => (dot(n, [0.6, 0.8, 0.0]) + 1.0) * s.stripes,
            // The angle of the face to the pull direction (z), degrees.
            Mode::Draft => n[2].clamp(-1.0, 1.0).asin().to_degrees(),
            // The normal curvature along each side, from how the normals turn along it.
            Mode::Curvature => {
                let k = |i: usize, j: usize| {
                    let d = sub(position(lod, j), position(lod, i));
                    let len2 = dot(d, d);
                    if len2 <= 1e-300 {
                        0.0
                    } else {
                        dot(sub(normal(lod, j), normal(lod, i)), d) / len2
                    }
                };
                let ks = [k(a, b), k(b, c), k(c, a)];
                let (lo, hi) = (ks.iter().copied().fold(f64::INFINITY, f64::min), ks.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                match s.curvature {
                    0 => lo * hi,
                    1 => (ks[0] + ks[1] + ks[2]) / 3.0,
                    2 => hi,
                    _ => lo,
                }
            }
        };
        values.push(value);
    }
    let colours: Vec<Option<[f32; 4]>> = match mode {
        Mode::Zebra => values
            .iter()
            .map(|v| Some(if (v.floor() as i64).rem_euclid(2) == 0 { [0.05, 0.05, 0.05, 1.0] } else { [0.95, 0.95, 0.95, 1.0] }))
            .collect(),
        Mode::Draft => {
            let [lo, hi] = s.draft;
            let mid = 0.5 * (lo + hi);
            let half = (0.5 * (hi - lo)).max(1e-9);
            values.iter().map(|v| Some(ramp((v - mid) / half))).collect()
        }
        Mode::Curvature => {
            let scale = values.iter().fold(0.0_f64, |m, v| m.max(v.abs())).max(1e-12);
            values.iter().map(|v| Some(ramp(v / scale))).collect()
        }
    };
    lod.triangle_colors = colours;
}

/// Colours the meshes of `set` by the analysis shown; nothing when none is.
pub fn apply(set: &mut MeshLodSet) {
    let s = settings();
    let Some(mode) = s.mode else {
        return;
    };
    for lod in &mut set.lods {
        if lod.indices.len() >= 3 && lod.verts.len() == lod.normals.len() {
            colour_lod(lod, mode, &s);
        }
    }
}
