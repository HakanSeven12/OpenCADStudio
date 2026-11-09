//! Assembles a kernel B-rep from the topology an exchange file spells out:
//! vertices, edges along their curves, faces bounded by loops of those
//! edges, shells of faces. The STEP and IGES readers share it.
//!
//! Spline curves and surfaces that are exactly lines, circles, planes,
//! spheres, cylinders, cones or tori are kept as those shapes: many systems write
//! every one of them as a spline. With [`Builder::closed_only`] only lines,
//! planes and the shapes that close on themselves (a whole circle, a whole
//! turn round an axis) are, the others staying splines.

use std::f64::consts::TAU;

use kernel::brep::{
    Body, Coedge, Curve3, Edge, EdgeKey, Face, Loop, Lump, Provenance, Shell, Surface, Vertex,
    VertexKey,
};
use kernel::space::Vec3;

/// A face as the file gives it: its surface, whether its outward normal
/// agrees with the surface's, and its loops — the outer one first — as
/// edges with the sense each is run in.
pub struct FaceSpec {
    pub surface: Surface,
    pub forward: bool,
    pub loops: Vec<Vec<(EdgeKey, bool)>>,
}

pub struct Builder {
    pub body: Body,
    /// Keep a spline circle, cylinder, cone, sphere or torus as that shape
    /// only when it closes on itself (IGES).
    pub closed_only: bool,
}

/// Whether a curve repeats, and over how long a parameter run.
fn period(curve: &Curve3) -> Option<f64> {
    match curve {
        Curve3::Circle(_) | Curve3::Ellipse(_) => Some(TAU),
        Curve3::Nurbs(nurbs) if nurbs.periodicity() => {
            let (from, to) = nurbs.domain();
            Some(to - from)
        }
        _ => None,
    }
}

/// The parameter of a point on (or next to) a curve.
fn parameter(curve: &Curve3, point: [f64; 3]) -> Option<f64> {
    Some(match curve {
        Curve3::Line(line) => {
            let direction = Vec3::from(line.direction);
            (Vec3::from(point) - Vec3::from(line.origin)).dot(direction)
                / direction.length_squared().max(f64::MIN_POSITIVE)
        }
        Curve3::Circle(circle) => {
            let [x, y] = circle.plane.project(point)?;
            y.atan2(x).rem_euclid(TAU)
        }
        Curve3::Ellipse(ellipse) => {
            let [x, y] = ellipse.plane.project(point)?;
            (y / ellipse.minor_radius)
                .atan2(x / ellipse.major_radius)
                .rem_euclid(TAU)
        }
        Curve3::Nurbs(nurbs) => nurbs.parameter_at(point),
        Curve3::PlanarSpline { .. } => return None,
    })
}

/// The same curve run the other way.
fn reversed(curve: Curve3) -> Option<Curve3> {
    Some(match curve {
        Curve3::Line(mut line) => {
            line.direction = (-Vec3::from(line.direction)).to_array();
            Curve3::Line(line)
        }
        Curve3::Nurbs(nurbs) => Curve3::Nurbs(nurbs.reversed()?),
        _ => return None,
    })
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            body: Body::new(),
            closed_only: false,
        }
    }

    pub fn vertex(&mut self, point: [f64; 3]) -> VertexKey {
        self.body.vertices.insert(Vertex {
            point,
            provenance: Provenance::Synthesized,
        })
    }

    fn point(&self, vertex: VertexKey) -> Option<[f64; 3]> {
        self.body.vertices.get(vertex).map(|v| v.point)
    }

    /// An edge along `curve` from `start` to `end`, which the curve runs
    /// through in that order. A curve that turns out to run the other way
    /// (or an open one handed its ends swapped) is reversed to match.
    pub fn edge(&mut self, curve: Curve3, start: VertexKey, end: VertexKey) -> Option<EdgeKey> {
        let (a, b) = (self.point(start)?, self.point(end)?);
        let mut curve = match curve {
            Curve3::Nurbs(nurbs) => match kernel::brep::analytic_curve(&nurbs) {
                Some(line @ Curve3::Line(_)) => line,
                Some(analytic) if !self.closed_only || nurbs.is_closed() => analytic,
                _ => Curve3::Nurbs(nurbs),
            },
            other => other,
        };
        let mut t0 = parameter(&curve, a)?;
        let mut t1 = parameter(&curve, b)?;
        match period(&curve) {
            Some(period) => {
                if let Curve3::Nurbs(nurbs) = &curve {
                    let (from, _) = nurbs.domain();
                    if start == end {
                        t0 = from;
                    }
                }
                if start == end {
                    t1 = t0 + period;
                } else {
                    while t1 <= t0 + 1e-12 {
                        t1 += period;
                    }
                }
            }
            None => {
                if start == end {
                    // A clamped curve closing on itself: its whole domain.
                    if let Curve3::Nurbs(nurbs) = &curve {
                        (t0, t1) = nurbs.domain();
                    }
                } else if t1 < t0 {
                    curve = reversed(curve)?;
                    t0 = parameter(&curve, a)?;
                    t1 = parameter(&curve, b)?;
                }
            }
        }
        if !(t1 > t0) {
            return None;
        }
        let curve = self.body.curves.insert(curve);
        Some(self.body.edges.insert(Edge {
            curve,
            start_parameter: t0,
            end_parameter: t1,
            start,
            end,
            coedges: Vec::new(),
            provenance: Provenance::Synthesized,
        }))
    }

    /// A lump of shells — the outer one first, then voids — each a list of
    /// faces. `None` when a face has no surface or an edge has gone.
    pub fn lump(&mut self, shells: Vec<Vec<FaceSpec>>) -> Option<()> {
        let lump = self.body.lumps.insert(Lump {
            shells: Vec::new(),
            provenance: Provenance::Synthesized,
        });
        for faces in shells {
            let shell = self.body.shells.insert(Shell {
                faces: Vec::new(),
                owner: lump,
                provenance: Provenance::Synthesized,
            });
            for spec in faces {
                let (surface, forward) = match spec.surface {
                    Surface::Nurbs(nurbs) => match kernel::brep::analytic_surface(&nurbs) {
                        Some((analytic, agrees))
                            if !self.closed_only
                                || matches!(analytic, Surface::Plane(_))
                                || nurbs.periodicity().contains(&true) =>
                        {
                            (analytic, spec.forward == agrees)
                        }
                        _ => (Surface::Nurbs(nurbs), spec.forward),
                    },
                    other => (other, spec.forward),
                };
                let surface = self.body.surfaces.insert(surface);
                let face = self.body.faces.insert(Face {
                    surface,
                    forward,
                    loops: Vec::new(),
                    owner: shell,
                    provenance: Provenance::Synthesized,
                });
                for ring in spec.loops {
                    let key = self.body.loops.insert(Loop {
                        coedges: Vec::new(),
                        owner: face,
                        provenance: Provenance::Synthesized,
                    });
                    let mut coedges = Vec::with_capacity(ring.len());
                    for (edge, forward) in ring {
                        let coedge = self.body.coedges.insert(Coedge {
                            edge,
                            forward,
                            pcurve: None,
                            owner: key,
                            provenance: Provenance::Synthesized,
                        });
                        self.body.edges.get_mut(edge)?.coedges.push(coedge);
                        coedges.push(coedge);
                    }
                    self.body.loops.get_mut(key)?.coedges = coedges;
                    self.body.faces.get_mut(face)?.loops.push(key);
                }
                self.body.shells.get_mut(shell)?.faces.push(face);
            }
            self.body.lumps.get_mut(lump)?.shells.push(shell);
        }
        self.body.roots.push(lump);
        Some(())
    }
}
