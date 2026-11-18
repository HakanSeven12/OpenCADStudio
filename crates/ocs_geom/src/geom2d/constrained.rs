use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::cell::Cell;

thread_local! {
    static SPLIT_CROSSINGS: Cell<bool> = const { Cell::new(false) };
}

/// Runs `fill` with boundary chords that cross split where they cross,
/// rather than the triangulation refused. Rings whose curves only touch — a
/// hole tangent to its face's rim — cross as chords near the touch however
/// finely they are sampled; split, the parity test still keeps the right
/// side, a sliver at the touch aside. A last resort, for a face nothing else
/// meshes.
pub fn splitting_crossings<T>(fill: impl FnOnce() -> T) -> T {
    let before = SPLIT_CROSSINGS.with(|flag| flag.replace(true));
    let filled = fill();
    SPLIT_CROSSINGS.with(|flag| flag.set(before));
    filled
}

#[derive(Clone, Copy, Debug)]
struct ParameterVertex {
    position: Point2<f64>,
    parameters: [f64; 2],
}

impl HasPosition for ParameterVertex {
    type Scalar = f64;

    fn position(&self) -> Point2<Self::Scalar> {
        self.position
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ConstrainedTriangle {
    pub parameters: [[f64; 2]; 3],
    pub constraints: [bool; 3],
}

pub struct ConstrainedMesh {
    triangulation: ConstrainedDelaunayTriangulation<ParameterVertex>,
    rings: Vec<Vec<[f64; 2]>>,
    /// Each ring's box, `[min x, max x, min y, max y]`: a face with a hundred
    /// holes tests a point against the one or two rings around it, not all.
    ring_boxes: Vec<[f64; 4]>,
    origin: [f64; 2],
    scale: [f64; 2],
}

impl ConstrainedMesh {
    pub fn new(rings: &[Vec<[f64; 2]>]) -> Option<Self> {
        if rings.is_empty() || rings.iter().any(|ring| ring.len() < 3) {
            return None;
        }
        let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for point in rings.iter().flatten() {
            for axis in 0..2 {
                bounds[axis][0] = bounds[axis][0].min(point[axis]);
                bounds[axis][1] = bounds[axis][1].max(point[axis]);
            }
        }
        let origin = [bounds[0][0], bounds[1][0]];
        let scale = [
            bounds[0][1] - bounds[0][0],
            bounds[1][1] - bounds[1][0],
        ];
        if scale.iter().any(|value| !value.is_finite() || *value <= 0.0) {
            return None;
        }

        let mut mesh = Self {
            triangulation: ConstrainedDelaunayTriangulation::new(),
            rings: rings.to_vec(),
            ring_boxes: rings
                .iter()
                .map(|ring| {
                    ring.iter().fold(
                        [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY],
                        |b, p| [b[0].min(p[0]), b[1].max(p[0]), b[2].min(p[1]), b[3].max(p[1])],
                    )
                })
                .collect(),
            origin,
            scale,
        };
        let mut handles = Vec::with_capacity(rings.len());
        for ring in rings {
            let mut ring_handles = Vec::with_capacity(ring.len());
            for parameters in ring {
                let vertex = mesh.vertex(*parameters);
                let handle = mesh.triangulation.insert(vertex).ok()?;
                if ring_handles.last() == Some(&handle) {
                    return None;
                }
                ring_handles.push(handle);
            }
            if ring_handles.first() == ring_handles.last() {
                ring_handles.pop();
            }
            if ring_handles.len() < 3 {
                return None;
            }
            handles.push(ring_handles);
        }
        let (origin, scale) = (mesh.origin, mesh.scale);
        let splitting = SPLIT_CROSSINGS.with(Cell::get);
        for ring in handles {
            for index in 0..ring.len() {
                let from = ring[index];
                let to = ring[(index + 1) % ring.len()];
                if from == to {
                    return None;
                }
                if !mesh.triangulation.try_add_constraint(from, to).is_empty() {
                    continue;
                }
                if !splitting
                    || mesh
                        .triangulation
                        .add_constraint_and_split(from, to, |position| ParameterVertex {
                            parameters: [
                                origin[0] + position.x * scale[0],
                                origin[1] + position.y * scale[1],
                            ],
                            position,
                        })
                        .is_empty()
                {
                    return None;
                }
            }
        }
        Some(mesh)
    }

    pub fn triangles(&self) -> Vec<ConstrainedTriangle> {
        self.triangulation
            .inner_faces()
            .filter_map(|face| {
                let vertices = face.vertices();
                let parameters = vertices.map(|vertex| vertex.data().parameters);
                let centre = [
                    parameters.iter().map(|point| point[0]).sum::<f64>() / 3.0,
                    parameters.iter().map(|point| point[1]).sum::<f64>() / 3.0,
                ];
                self.inside(centre).then(|| ConstrainedTriangle {
                    parameters,
                    constraints: face
                        .adjacent_edges()
                        .map(|edge| edge.as_undirected().is_constraint_edge()),
                })
            })
            .collect()
    }

    pub fn insert(&mut self, parameters: [f64; 2]) -> Option<bool> {
        let before = self.triangulation.num_vertices();
        let vertex = self.vertex(parameters);
        self.triangulation.insert(vertex).ok()?;
        Some(self.triangulation.num_vertices() > before)
    }

    pub fn constrain(&mut self, from: [f64; 2], to: [f64; 2]) -> Option<usize> {
        let before = self.triangulation.num_vertices();
        let from = self.triangulation.insert(self.vertex(from)).ok()?;
        let to = self.triangulation.insert(self.vertex(to)).ok()?;
        if from != to {
            let origin = self.origin;
            let scale = self.scale;
            let edges = self
                .triangulation
                .add_constraint_and_split(from, to, |position| ParameterVertex {
                    parameters: [
                        origin[0] + position.x * scale[0],
                        origin[1] + position.y * scale[1],
                    ],
                    position,
                });
            if edges.is_empty() {
                return None;
            }
        }
        Some(self.triangulation.num_vertices() - before)
    }

    fn vertex(&self, parameters: [f64; 2]) -> ParameterVertex {
        let step = f64::EPSILON;
        let normalized = [0, 1].map(|axis| {
            let value = (parameters[axis] - self.origin[axis]) / self.scale[axis];
            (value / step).round() * step
        });
        ParameterVertex {
            position: Point2::new(normalized[0], normalized[1]),
            parameters,
        }
    }

    fn inside(&self, point: [f64; 2]) -> bool {
        self.rings
            .iter()
            .zip(&self.ring_boxes)
            .filter(|(_, b)| {
                point[0] >= b[0] && point[0] <= b[1] && point[1] >= b[2] && point[1] <= b[3]
            })
            .filter(|(ring, _)| ring_contains(ring, point))
            .count()
            % 2
            == 1
    }
}

fn ring_contains(ring: &[[f64; 2]], point: [f64; 2]) -> bool {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .take(ring.len())
        .fold(false, |inside, (from, to)| {
            let crosses = (from[1] > point[1]) != (to[1] > point[1])
                && point[0]
                    < (to[0] - from[0]) * (point[1] - from[1]) / (to[1] - from[1])
                        + from[0];
            inside ^ crosses
        })
}
