//! A shaded mesh's view-dependent outline: the sides between two of its flat triangles that
//! face opposite ways from the eye, and the contour through its smooth triangles, where their
//! vertex normals turn from facing the eye to facing away.

use std::collections::HashMap;

/// What a mesh's outline is found from, for any view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SilhouetteSource {
    sides: Vec<Side>,
    triangles: Vec<Smooth>,
    precision: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Side {
    positions: [[f64; 3]; 2],
    normals: [[f64; 3]; 2],
}

#[derive(Debug, Clone, PartialEq)]
struct Smooth {
    positions: [[f64; 3]; 3],
    normals: [[f64; 3]; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
    let l = dot(a, a).sqrt();
    (l.is_finite() && l > 1e-300).then(|| a.map(|x| x / l))
}

impl SilhouetteSource {
    /// The outline source of a mesh whose triangle `t` lies on face `groups[t]`: a side is only
    /// an outline between two triangles of one face (a face's own edges are drawn anyway).
    pub fn of_mesh(positions: &[[f64; 3]], normals: &[[f64; 3]], triangles: &[[usize; 3]], groups: &[u32]) -> Self {
        let scale = positions.iter().flatten().map(|v| v.abs()).fold(1.0, f64::max);
        let precision = (scale * f64::EPSILON * 1024.0).max(1e-12);
        let origin = positions.first().copied().unwrap_or([0.0; 3]);
        let key = |p: [f64; 3]| [0, 1, 2].map(|k| ((p[k] - origin[k]) / precision).round() as i64);
        let mut open: HashMap<(u32, [i64; 3], [i64; 3]), ([f64; 3], [[f64; 3]; 2])> = HashMap::new();
        let mut out = SilhouetteSource {
            precision,
            ..Self::default()
        };
        for (index, triangle) in triangles.iter().enumerate() {
            let Some(group) = groups.get(index).copied() else { continue };
            let corners = triangle.map(|v| positions[v]);
            let Some(flat) = unit(cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]))) else {
                continue;
            };
            let vertex_normals = triangle.map(|v| normals.get(v).and_then(|n| unit(*n)).unwrap_or(flat));
            let smooth = [[0, 1], [1, 2], [2, 0]].into_iter().any(|[a, b]| {
                let d = sub(vertex_normals[a], vertex_normals[b]);
                dot(d, d).sqrt() > 1e-10
            });
            if smooth {
                out.triangles.push(Smooth {
                    positions: corners,
                    normals: vertex_normals,
                });
                continue;
            }
            for [from, to] in [[0, 1], [1, 2], [2, 0]] {
                let (mut a, mut b) = (key(corners[from]), key(corners[to]));
                let mut segment = [corners[from], corners[to]];
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                    segment.swap(0, 1);
                }
                if let Some((other, previous)) = open.remove(&(group, a, b)) {
                    out.sides.push(Side {
                        positions: previous,
                        normals: [other, flat],
                    });
                } else {
                    open.insert((group, a, b), (flat, segment));
                }
            }
        }
        out
    }

    /// The outline seen along `view`, as segment end pairs.
    pub fn outline(&self, view: [f64; 3]) -> Vec<[f64; 3]> {
        let Some(view) = unit(view) else {
            return Vec::new();
        };
        let seed = if view[0].abs() < 0.8 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let tangent = unit(cross(view, seed)).unwrap_or(seed);
        let bitangent = cross(view, tangent);
        // Keep a contour from landing exactly on a mesh vertex.
        let contour = unit([0, 1, 2].map(|k| view[k] + tangent[k] * 1e-7 + bitangent[k] * 6.180_339_887e-8)).unwrap_or(view);
        let mut out = Vec::new();
        for side in &self.sides {
            let signs = side.normals.map(|n| dot(n, contour).signum());
            if signs[0] != signs[1] {
                out.extend(side.positions);
            }
        }
        for triangle in &self.triangles {
            let values = triangle.normals.map(|n| dot(n, contour));
            let mut crossings = Vec::with_capacity(2);
            for [from, to] in [[0, 1], [1, 2], [2, 0]] {
                if values[from].is_sign_positive() == values[to].is_sign_positive() {
                    continue;
                }
                let t = values[from] / (values[from] - values[to]);
                let (a, b) = (triangle.positions[from], triangle.positions[to]);
                crossings.push([0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t));
            }
            if let [a, b] = crossings.as_slice() {
                out.extend([*a, *b]);
            }
        }
        out
    }

    /// This source moved by the affine map whose columns are `vectors`, then by `origin`.
    pub fn transformed(&self, vectors: [[f64; 3]; 3], origin: [f64; 3]) -> Option<SilhouetteSource> {
        if origin.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let lengths = vectors.map(|v| dot(v, v).sqrt());
        if lengths.iter().any(|l| !l.is_finite() || *l <= 0.0) {
            return None;
        }
        let units = [0, 1, 2].map(|k| vectors[k].map(|x| x / lengths[k]));
        let determinant = dot(units[0], cross(units[1], units[2]));
        if !determinant.is_finite() || determinant.abs() <= 1e-12 {
            return None;
        }
        let normal_vectors = [
            cross(units[1], units[2]).map(|x| x / (determinant * lengths[0])),
            cross(units[2], units[0]).map(|x| x / (determinant * lengths[1])),
            cross(units[0], units[1]).map(|x| x / (determinant * lengths[2])),
        ];
        let point = |p: [f64; 3]| [0, 1, 2].map(|k| origin[k] + vectors[0][k] * p[0] + vectors[1][k] * p[1] + vectors[2][k] * p[2]);
        let normal = |n: [f64; 3]| unit([0, 1, 2].map(|k| normal_vectors[0][k] * n[0] + normal_vectors[1][k] * n[1] + normal_vectors[2][k] * n[2]));
        let mut out = self.clone();
        for side in &mut out.sides {
            side.positions = side.positions.map(point);
            side.normals = [normal(side.normals[0])?, normal(side.normals[1])?];
        }
        for triangle in &mut out.triangles {
            triangle.positions = triangle.positions.map(point);
            triangle.normals = [normal(triangle.normals[0])?, normal(triangle.normals[1])?, normal(triangle.normals[2])?];
        }
        out.precision *= vectors.iter().map(|v| dot(*v, *v)).sum::<f64>().sqrt();
        Some(out)
    }
}
