//! Geometric checks of a body: whether its curves, surfaces and vertices
//! agree with one another, beyond the topology [`Body::validate`] checks.
//!
//! The tests are those of Solidcore's validity check (NVIDIA, Apache-2.0;
//! `docs/validity-and-tolerances.md` §4 and
//! `topology/brep/validation/assert_valid/`), written on this kernel's
//! queries: an edge ends on its vertices and is not shorter than the
//! tolerance, it lies on each of its faces at every one of twenty samples,
//! a stored pcurve traces its edge and does not jump a period, and a face's
//! inner loops lie inside its outer one and cross no other loop. Each flaw is
//! one readable line naming what is wrong and by how much.

use super::geometry::Surface;
use super::topology::{Body, EdgeKey, FaceKey};
use ocs_geom::space::Vec3;

/// How many points along an edge are measured against its faces.
const SAMPLES: usize = 20;

/// Everything geometrically wrong with `body` at `tolerance`, one line each.
pub fn geometry_flaws(body: &Body, tolerance: f64) -> Vec<String> {
    let mut flaws = Vec::new();
    for (key, edge) in body.edges.iter() {
        let Some(curve) = body.curves.get(edge.curve) else {
            continue;
        };
        // Its ends on its vertices.
        for (parameter, vertex, which) in [
            (edge.start_parameter, edge.start, "start"),
            (edge.end_parameter, edge.end, "end"),
        ] {
            let Some(vertex) = body.vertices.get(vertex) else {
                continue;
            };
            let gap = Vec3::from(curve.point_at(parameter)).distance(Vec3::from(vertex.point));
            if gap > tolerance {
                flaws.push(format!("edge {key:?}: its {which} is {gap:.3e} from its vertex"));
            }
        }
        let points: Vec<[f64; 3]> = (0..=SAMPLES)
            .map(|index| {
                let share = index as f64 / SAMPLES as f64;
                curve.point_at(edge.start_parameter + (edge.end_parameter - edge.start_parameter) * share)
            })
            .collect();
        let length: f64 = points.windows(2).map(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1]))).sum();
        if length < tolerance && edge.start != edge.end {
            flaws.push(format!("edge {key:?}: {length:.3e} long, shorter than the tolerance"));
        }
        // On each face it bounds.
        for coedge in &edge.coedges {
            let Some(face) = face_of(body, *coedge) else {
                continue;
            };
            let Some(surface) = body.faces.get(face).and_then(|face| body.surfaces.get(face.surface)) else {
                continue;
            };
            let gap = points.iter().map(|point| surface.distance_to(*point).abs()).fold(0.0, f64::max);
            if gap > tolerance {
                flaws.push(format!("edge {key:?}: lies up to {gap:.3e} off face {face:?}"));
            }
            if let Some(flaw) = pcurve_flaw(body, key, *coedge, surface, tolerance) {
                flaws.push(flaw);
            }
        }
    }
    for face in body.face_keys() {
        flaws.extend(loop_flaws(body, face));
    }
    flaws
}

fn face_of(body: &Body, coedge: super::topology::CoedgeKey) -> Option<FaceKey> {
    Some(body.loops.get(body.coedges.get(coedge)?.owner)?.owner)
}

/// A stored pcurve that leaves its edge, runs the other way, or jumps
/// across a period of its surface.
fn pcurve_flaw(
    body: &Body,
    edge_key: EdgeKey,
    coedge_key: super::topology::CoedgeKey,
    surface: &Surface,
    tolerance: f64,
) -> Option<String> {
    let coedge = body.coedges.get(coedge_key)?;
    let pcurve = coedge.pcurve.as_ref()?;
    let edge = body.edges.get(edge_key)?;
    let curve = body.curves.get(edge.curve)?;
    let periods = super::pcurve::periods(surface);
    let mut previous: Option<[f64; 2]> = None;
    let mut worst = 0.0_f64;
    for index in 0..=SAMPLES {
        let uv = pcurve.point_at(index as f64 / SAMPLES as f64);
        if let Some(previous) = previous {
            for axis in 0..2 {
                if let Some(period) = periods[axis] {
                    if (uv[axis] - previous[axis]).abs() > period / 2.0 {
                        return Some(format!(
                            "coedge {coedge_key:?} of edge {edge_key:?}: its pcurve jumps across the surface's seam"
                        ));
                    }
                }
            }
        }
        previous = Some(uv);
        let on = Vec3::from(surface.point_at(uv[0], uv[1]));
        let back = Vec3::from(curve.point_at(curve.parameter_at(on.to_array())));
        worst = worst.max(on.distance(back));
    }
    if worst > tolerance {
        return Some(format!(
            "coedge {coedge_key:?} of edge {edge_key:?}: its pcurve strays {worst:.3e} from the edge"
        ));
    }
    // It starts where the coedge does.
    let (from, _) = body.coedge_vertices(coedge_key)?;
    let start = pcurve.point_at(0.0);
    let gap = Vec3::from(surface.point_at(start[0], start[1])).distance(Vec3::from(body.vertices.get(from)?.point));
    (gap > tolerance).then(|| {
        format!("coedge {coedge_key:?} of edge {edge_key:?}: its pcurve starts {gap:.3e} from the coedge's first vertex")
    })
}

/// Inner loops outside the outer one, and loops that cross, in the face's
/// parameters. Only on surfaces that do not wrap, where a loop is a plain
/// polygon there.
fn loop_flaws(body: &Body, face: FaceKey) -> Vec<String> {
    let Some(node) = body.faces.get(face) else {
        return Vec::new();
    };
    let Some(surface) = body.surfaces.get(node.surface) else {
        return Vec::new();
    };
    if node.loops.len() < 2 || super::pcurve::periods(surface) != [None, None] {
        return Vec::new();
    }
    let scale = super::bounds::face_bounds(body, face)
        .map(|bounds| Vec3::from(bounds.min).distance(Vec3::from(bounds.max)))
        .unwrap_or(1.0);
    let Some(parts) = super::pcurve::face_boundary_parts(body, face, 1e-9 * scale.max(1.0)) else {
        return Vec::new();
    };
    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut parts = parts.into_iter();
    for loop_key in &node.loops {
        let Some(ring) = body.loops.get(*loop_key) else {
            return Vec::new();
        };
        let mut polygon = Vec::new();
        for (_, piece) in parts.by_ref().take(ring.coedges.len()) {
            polygon.extend((0..16).map(|index| piece.point_at(index as f64 / 16.0)));
        }
        rings.push(polygon);
    }
    let mut flaws = Vec::new();
    for (index, ring) in rings.iter().enumerate().skip(1) {
        if !ring.iter().all(|point| inside(&rings[0], *point)) {
            flaws.push(format!("face {face:?}: inner loop {index} is not inside the outer loop"));
        }
    }
    for first in 0..rings.len() {
        for second in first + 1..rings.len() {
            if crosses(&rings[first], &rings[second]) {
                flaws.push(format!("face {face:?}: loops {first} and {second} cross"));
            } else if first > 0 && inside(&rings[first], rings[second][0]) {
                flaws.push(format!("face {face:?}: inner loop {second} lies inside inner loop {first}"));
            }
        }
    }
    flaws
}

/// Even-odd point in polygon.
fn inside(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    let mut odd = false;
    for (index, a) in polygon.iter().enumerate() {
        let b = polygon[(index + 1) % polygon.len()];
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let x = a[0] + (point[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if point[0] < x {
                odd = !odd;
            }
        }
    }
    odd
}

/// Whether two closed polygons' edges properly cross.
fn crosses(one: &[[f64; 2]], two: &[[f64; 2]]) -> bool {
    let side = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let segments = |polygon: &[[f64; 2]]| {
        (0..polygon.len()).map(|index| (polygon[index], polygon[(index + 1) % polygon.len()])).collect::<Vec<_>>()
    };
    let others = segments(two);
    segments(one).iter().any(|(a, b)| {
        others.iter().any(|(c, d)| {
            side(*a, *b, *c) * side(*a, *b, *d) < 0.0 && side(*c, *d, *a) * side(*c, *d, *b) < 0.0
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make;

    #[test]
    fn sound_bodies_have_no_geometric_flaws() {
        let block = make::cuboid([0.0; 3], [4.0, 4.0, 2.0]).unwrap();
        let bore = make::cylinder([2.0, 2.0, -1.0], 1.0, 4.0).unwrap();
        let holed = crate::combine(block, bore, crate::Operation::Difference, 1e-9).unwrap();
        for body in [holed, make::sphere([1.0, 2.0, 3.0], 2.0).unwrap(), make::torus([0.0; 3], 3.0, 1.0).unwrap()] {
            assert!(geometry_flaws(&body, 1e-7).is_empty(), "{:?}", geometry_flaws(&body, 1e-7));
        }
    }

    #[test]
    fn a_vertex_moved_off_its_edges_is_reported() {
        let mut body = make::cuboid([0.0; 3], [1.0, 1.0, 1.0]).unwrap();
        let key = body.vertices.iter().next().unwrap().0;
        body.vertices.get_mut(key).unwrap().point[2] += 0.01;
        let flaws = geometry_flaws(&body, 1e-7);
        assert!(flaws.iter().any(|flaw| flaw.contains("from its vertex")), "{flaws:?}");
    }

    #[test]
    fn a_moved_face_leaves_its_edges_off_it() {
        let mut body = make::cuboid([0.0; 3], [1.0, 1.0, 1.0]).unwrap();
        let face = crate::face_at_point(&body, [0.5, 0.5, 1.0]).unwrap();
        let surface = body.faces.get(face).unwrap().surface;
        if let Some(Surface::Plane(plane)) = body.surfaces.get_mut(surface) {
            plane.origin[2] += 0.01;
        }
        let flaws = geometry_flaws(&body, 1e-7);
        assert_eq!(flaws.iter().filter(|flaw| flaw.contains("off face")).count(), 4, "{flaws:?}");
    }
}
