// Floor-plan geometry adapted from Open BIM Suite (v2026.46.5, src/drawing.rs,
// originally LGPL-3.0-or-later). This adaptation is distributed under GPL-3.0
// under the option in LGPL-3.0 section 2(b).
// https://github.com/OpenAEC-Foundation/open-bim-suite/tree/v2026.46.5

use super::Mesh;
use std::collections::HashMap;

pub(super) type Segment = [[f32; 2]; 2];
pub(super) struct SourceSegment {
    pub points: Segment,
    pub sources: Vec<u32>,
}

struct SourceEdge {
    points: [[f32; 3]; 2],
    sources: Vec<u32>,
}

const CREASE_DEGREES: f32 = 20.0;

pub(super) fn plan_at(meshes: &[Mesh], low: f32, high: f32) -> [Vec<SourceSegment>; 2] {
    let cut = meshes
        .iter()
        .flat_map(|mesh| {
            mesh.triangles().filter_map(move |corners| {
                let points = cross(corners, high)?;
                Some(SourceSegment {
                    points,
                    sources: vec![mesh.source],
                })
            })
        })
        .collect();
    let map = |p: [f32; 3]| [p[0], p[1], high - p[2]];
    let depth = high - low;
    let faces: Vec<_> = meshes
        .iter()
        .flat_map(Mesh::triangles)
        .map(|triangle| triangle.map(map))
        .filter(|c| {
            c[0][2].max(c[1][2]).max(c[2][2]) > 0.0 && c[0][2].min(c[1][2]).min(c[2][2]) < depth
        })
        .collect();
    [
        cut,
        visible(&faces, &feature_edges(meshes, map, 0.0, depth)),
    ]
}

/// An edge seen along its third coordinate: nothing when it is seen end on.
fn flat([a, b]: [[f32; 3]; 2]) -> Option<Segment> {
    ((b[0] - a[0]).hypot(b[1] - a[1]) > 1e-4).then_some([[a[0], a[1]], [b[0], b[1]]])
}

/// A hasher for the keys of edges (whole numbers): mixing them is enough, and several times as
/// fast as the standard one on the millions of edges of a large model.
#[derive(Default)]
struct Mix(u64);

impl std::hash::Hasher for Mix {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

fn feature_edges(
    meshes: &[Mesh],
    map: impl Fn([f32; 3]) -> [f32; 3],
    low: f32,
    high: f32,
) -> Vec<SourceEdge> {
    // edges by their ends rounded to 0.1 mm, so faces that do not share vertices still meet
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    let crease = CREASE_DEGREES.to_radians().cos();
    // Keep the first face normal, the face count, and whether there is a crease.
    struct EdgeFaces {
        points: [[f32; 3]; 2],
        normal: [f32; 3],
        faces: u32,
        sharp: bool,
        sources: Vec<u32>,
    }
    let mut edges: HashMap<([i64; 3], [i64; 3]), EdgeFaces, std::hash::BuildHasherDefault<Mix>> =
        HashMap::default();
    for mesh in meshes {
        for triangle in mesh.triangles() {
            let c = triangle.map(&map);
            let (lo, hi) = (
                c[0][2].min(c[1][2]).min(c[2][2]),
                c[0][2].max(c[1][2]).max(c[2][2]),
            );
            if hi < low || lo > high {
                continue;
            }
            let (u, v) = (sub(c[1], c[0]), sub(c[2], c[0]));
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len <= 1e-12 {
                continue;
            }
            let n = n.map(|x| x / len);
            for k in 0..3 {
                let (a, b) = (c[k], c[(k + 1) % 3]);
                let (ka, kb) = (key(a), key(b));
                let id = if ka <= kb { (ka, kb) } else { (kb, ka) };
                let entry = edges.entry(id).or_insert_with(|| EdgeFaces {
                    points: [a, b],
                    normal: n,
                    faces: 0,
                    sharp: false,
                    sources: Vec::new(),
                });
                if !entry.sources.contains(&mesh.source) {
                    entry.sources.push(mesh.source);
                }
                entry.faces += 1;
                if entry.normal[0] * n[0] + entry.normal[1] * n[1] + entry.normal[2] * n[2] < crease
                {
                    entry.sharp = true;
                }
            }
        }
    }
    let mut out = Vec::new();
    for EdgeFaces {
        points: [a, b],
        faces,
        sharp,
        sources,
        ..
    } in edges.into_values()
    {
        // the part of the edge between low and high
        let (za, zb) = (a[2], b[2]);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        if (za - zb).abs() > 1e-9 {
            let (ta, tb) = ((low - za) / (zb - za), (high - za) / (zb - za));
            t0 = t0.max(ta.min(tb));
            t1 = t1.min(ta.max(tb));
        } else if za < low || za > high {
            continue;
        }
        if t0 >= t1 {
            continue;
        }
        let at = |t: f32| {
            [
                a[0] + t * (b[0] - a[0]),
                a[1] + t * (b[1] - a[1]),
                a[2] + t * (b[2] - a[2]),
            ]
        };
        if faces == 1 || faces > 1 && sharp {
            out.push(SourceEdge {
                points: [at(t0), at(t1)],
                sources,
            });
        }
    }
    out
}

/// How many pixels the longer side of a section's depth image has.
const DEPTH_PIXELS: f32 = 2048.0;

/// Walk edges through a raster containing the nearest surface at each pixel.
fn visible(faces: &[[[f32; 3]; 3]], edges: &[SourceEdge]) -> Vec<SourceSegment> {
    let corners = faces
        .iter()
        .flatten()
        .chain(edges.iter().flat_map(|edge| &edge.points));
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for c in corners {
        lo = [lo[0].min(c[0]), lo[1].min(c[1])];
        hi = [hi[0].max(c[0]), hi[1].max(c[1])];
    }
    if !(lo[0] <= hi[0] && lo[1] <= hi[1]) {
        return Vec::new();
    }
    let pixel = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / DEPTH_PIXELS).max(1e-4);
    let (w, h) = (
        ((hi[0] - lo[0]) / pixel) as usize + 2,
        ((hi[1] - lo[1]) / pixel) as usize + 2,
    );
    let mut depth = vec![f32::INFINITY; w * h];
    for face in faces {
        let polygon = in_front(face);
        for k in 1..polygon.len().saturating_sub(1) {
            draw_depth(
                &mut depth,
                (w, h),
                lo,
                pixel,
                [polygon[0], polygon[k], polygon[k + 1]],
            );
        }
    }
    // an edge lies on its own faces: it shows unless something is clearly nearer
    let slack = 0.02 + 4.0 * pixel;
    let at = |p: [f32; 3]| {
        let (x, y) = (
            ((p[0] - lo[0]) / pixel) as usize,
            ((p[1] - lo[1]) / pixel) as usize,
        );
        depth[y.min(h - 1) * w + x.min(w - 1)]
    };
    let mut out = Vec::new();
    for edge in edges {
        let [a, b] = edge.points;
        let sourced = |p, q| {
            flat([p, q]).map(|points| SourceSegment {
                points,
                sources: edge.sources.clone(),
            })
        };
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / pixel * 1.5).ceil() as usize).max(1);
        let point = |k: usize| {
            let t = k as f32 / steps as f32;
            [
                a[0] + t * (b[0] - a[0]),
                a[1] + t * (b[1] - a[1]),
                a[2] + t * (b[2] - a[2]),
            ]
        };
        // the run being walked: where it began, and whether it is seen
        let mut run: Option<([f32; 3], bool)> = None;
        let mut last = a;
        for k in 0..=steps {
            let p = point(k);
            let seen = p[2] <= at(p) + slack;
            match run {
                None => run = Some((p, seen)),
                Some((s, was)) if was != seen => {
                    if was {
                        out.extend(sourced(s, last));
                    }
                    run = Some((last, seen));
                }
                _ => {}
            }
            last = p;
        }
        if let Some((s, true)) = run {
            out.extend(sourced(s, b));
        }
    }
    out
}

/// The part of a triangle at depth 0 or more (a polygon of 0, 3 or 4 corners).
fn in_front(t: &[[f32; 3]; 3]) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(4);
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        if a[2] >= 0.0 {
            out.push(a);
        }
        if (a[2] >= 0.0) != (b[2] >= 0.0) {
            let s = a[2] / (a[2] - b[2]);
            out.push([a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1]), 0.0]);
        }
    }
    out
}

/// Draws a triangle (u, v, depth) into the depth image, keeping the nearest depth per pixel.
fn draw_depth(
    depth: &mut [f32],
    (w, h): (usize, usize),
    lo: [f32; 2],
    pixel: f32,
    t: [[f32; 3]; 3],
) {
    let px = t.map(|c| [(c[0] - lo[0]) / pixel, (c[1] - lo[1]) / pixel, c[2]]);
    let area = (px[1][0] - px[0][0]) * (px[2][1] - px[0][1])
        - (px[2][0] - px[0][0]) * (px[1][1] - px[0][1]);
    if area.abs() < 1e-9 {
        return;
    }
    let x0 = px
        .iter()
        .map(|c| c[0])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let x1 = (px
        .iter()
        .map(|c| c[0])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as usize)
        .min(w - 1);
    let y0 = px
        .iter()
        .map(|c| c[1])
        .fold(f32::INFINITY, f32::min)
        .floor()
        .max(0.0) as usize;
    let y1 = (px
        .iter()
        .map(|c| c[1])
        .fold(f32::NEG_INFINITY, f32::max)
        .ceil() as usize)
        .min(h - 1);
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| {
        (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
    };
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (w0, w1, w2) = (
                edge(px[1], px[2], cx, cy) / area,
                edge(px[2], px[0], cx, cy) / area,
                edge(px[0], px[1], cx, cy) / area,
            );
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let z = w0 * px[0][2] + w1 * px[1][2] + w2 * px[2][2];
            let cell = &mut depth[y * w + x];
            if z < *cell {
                *cell = z;
            }
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Where a triangle crosses the plane at height `z`: a corner on the plane counts as above it,
/// so a triangle lying in the plane or touching it at one corner gives nothing.
fn cross(corners: [[f32; 3]; 3], z: f32) -> Option<Segment> {
    let above = corners.map(|c| c[2] >= z);
    let ups = above.iter().filter(|a| **a).count();
    if ups == 0 || ups == 3 {
        return None;
    }
    let mut points = Vec::with_capacity(2);
    for k in 0..3 {
        let (a, b) = (corners[k], corners[(k + 1) % 3]);
        if above[k] != above[(k + 1) % 3] {
            // from the same end whichever way the edge runs, so two triangles sharing it meet
            // in the very same point
            let (a, b) = if a[2] <= b[2] { (a, b) } else { (b, a) };
            let t = (z - a[2]) / (b[2] - a[2]);
            points.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
        }
    }
    let [a, b] = points[..] else { return None };
    (a != b).then_some([a, b])
}
