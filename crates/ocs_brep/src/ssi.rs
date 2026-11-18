//! Where two bounded surfaces meet, tangencies included.
//!
//! [`intersect`](super::intersect) answers for whole surfaces, in closed
//! form or not at all. A face is a bounded piece of a surface, and the pairs
//! no closed form covers are the hard ones: two cylinders at an angle, a
//! spline wall touching a plane along a line, two loops meeting at a point.
//! This module answers for two bounded pieces whatever they are, through the
//! `opencadkernel-ssi` engine: subdivision into rational Bézier pieces, the
//! implicit form of one surface composed with the other's pieces, and a
//! marcher for what is left. It finds crossing curves, curves of tangency,
//! isolated touch points and the points where branches cross, and splits the
//! curves there. What it cannot prove it refuses, never guesses.
//!
//! The engine works on rational B-spline patches. A plane, cylinder, cone,
//! sphere or torus becomes the exact rational patch of that surface over the
//! box asked for, carrying its implicit form; a spline surface goes in as it
//! is. Every point that comes back is given in the kernel's own parameters of
//! each surface (radians, the cone measured from its base circle), so it can
//! go straight into a face's parameter space.

use super::geometry::{OffsetSurface, Surface};
use ocs_geom::space::{NurbsSurface3, Vec3};

/// A bounded piece of a surface: the surface over `u` × `v` in its own
/// parameters.
#[derive(Debug, Clone, Copy)]
pub struct Bounded<'a> {
    /// The surface.
    pub surface: &'a Surface,
    /// Its `u` range.
    pub u: [f64; 2],
    /// Its `v` range.
    pub v: [f64; 2],
}

impl<'a> Bounded<'a> {
    /// A spline surface over its whole domain.
    pub fn whole(surface: &'a Surface) -> Option<Self> {
        let ((u0, u1), (v0, v1)) = surface.spline()?.domain();
        Some(Self { surface, u: [u0, u1], v: [v0, v1] })
    }
}

/// A point both surfaces share, with each one's parameters there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shared {
    /// The point in space.
    pub point: [f64; 3],
    /// Its parameters on the first surface.
    pub first: [f64; 2],
    /// Its parameters on the second surface.
    pub second: [f64; 2],
}

/// One branch of the intersection: points in order along it. A closed branch
/// does not repeat its first point.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    /// The points.
    pub points: Vec<Shared>,
    /// Whether it closes on itself.
    pub closed: bool,
}

/// Everything two bounded surfaces share.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meeting {
    /// The curves, crossing or tangent. Where two branches cross, each ends
    /// there.
    pub branches: Vec<Branch>,
    /// Isolated points where the surfaces touch.
    pub touches: Vec<Shared>,
    /// Where branches cross or meet.
    pub crossings: Vec<[f64; 3]>,
}

/// Why two surfaces' meeting was not built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A contact the engine could not prove: a tangency or a degenerate piece
    /// right at a crossing.
    Tangent(String),
    /// A surface the engine cannot take, or a box outside its domain.
    Unsupported(String),
    /// The marcher could not go on, or an arc did not end.
    Failed(String),
}

/// How the engine's parameters on a patch become the surface's own.
#[derive(Debug, Clone, Copy)]
enum Mapping {
    /// The same numbers.
    Same,
    /// Degrees to radians in `u`.
    DegreesU,
    /// Degrees to radians in both.
    Degrees,
    /// A cone flipped to grow from its apex: `u` mirrored and in degrees, `v`
    /// measured back from the apex at `apex`.
    Cone { apex: f64, flipped: bool },
    /// A spline whose `v` runs the other way: mirrored across `v0 + v1`.
    Reversed { sum: f64 },
}

impl Mapping {
    /// The engine's knot parameters as the surface's own, approximately for
    /// an analytic patch: a rational arc's knot is not its angle.
    fn back(self, [u, v]: [f64; 2]) -> [f64; 2] {
        match self {
            Self::Same => [u, v],
            Self::DegreesU => [u.to_radians(), v],
            Self::Degrees => [u.to_radians(), v.to_radians()],
            Self::Cone { apex, flipped: true } => [-u.to_radians(), apex - v],
            Self::Cone { apex, flipped: false } => [u.to_radians(), apex + v],
            Self::Reversed { sum } => [u, sum - v],
        }
    }
}

/// The engine's patch of a bounded surface, and how its parameters map back.
fn patch(piece: &Bounded) -> Result<(ssi::Patch, Mapping), Refusal> {
    use ssi::shapes;
    let degrees = |r: [f64; 2]| [r[0].to_degrees(), r[1].to_degrees()];
    Ok(match piece.surface {
        Surface::Plane(plane) => (
            shapes::plane(plane.origin, plane.x_axis, plane.y_axis, piece.u, piece.v),
            Mapping::Same,
        ),
        Surface::Cylinder(cylinder) => (
            shapes::cylinder(
                cylinder.base.origin,
                cylinder.base.x_axis,
                cylinder.base.y_axis,
                cylinder.radius,
                degrees(piece.u),
                piece.v,
            ),
            Mapping::DegreesU,
        ),
        Surface::Sphere(sphere) => (
            shapes::sphere(
                sphere.frame.origin,
                sphere.frame.x_axis,
                sphere.frame.y_axis,
                sphere.radius,
                degrees(piece.u),
                degrees(piece.v),
            ),
            Mapping::Degrees,
        ),
        Surface::Torus(torus) => (
            shapes::torus(
                torus.frame.origin,
                torus.frame.x_axis,
                torus.frame.y_axis,
                torus.major_radius,
                torus.minor_radius,
                degrees(piece.u),
                degrees(piece.v),
            ),
            Mapping::Degrees,
        ),
        Surface::Cone(cone) => {
            // The engine's cone grows from its apex along its axis; the
            // kernel's is measured from its base circle and narrows along
            // its axis for a positive half-angle. Same surface, read from the
            // apex, the axis turned round when it narrows.
            let slope = cone.half_angle.tan();
            if slope.abs() < 1e-12 {
                return Err(Refusal::Unsupported("a cone with no slope".into()));
            }
            let apex = cone.radius / slope;
            let normal = Vec3::from(
                cone.base
                    .normal()
                    .ok_or_else(|| Refusal::Unsupported("a cone without an axis".into()))?,
            );
            let tip = (Vec3::from(cone.base.origin) + normal * apex).to_array();
            let flipped = slope > 0.0;
            let (y, heights) = if flipped {
                let y = (Vec3::from(cone.base.y_axis) * -1.0).to_array();
                (y, [apex - piece.v[1], apex - piece.v[0]])
            } else {
                (cone.base.y_axis, [piece.v[0] - apex, piece.v[1] - apex])
            };
            // `shapes::cone` sweeps the whole turn: a part of it is cut down
            // to the box by clipping the answer.
            let patch = shapes::cone(tip, cone.base.x_axis, y, slope.abs().atan().to_degrees(), heights);
            (patch, Mapping::Cone { apex, flipped })
        }
        Surface::Nurbs(spline) | Surface::Offset(OffsetSurface { approximation: spline, .. }) => {
            spline_patch(spline)?
        }
    })
}

/// A spline surface as the engine's patch, over its whole domain.
fn spline_patch(spline: &NurbsSurface3) -> Result<(ssi::Patch, Mapping), Refusal> {
    let (p, q) = spline.degrees();
    let (u_knots, v_knots) = spline.knots();
    let net = spline.control_points();
    let weights = spline.weights();
    let rows = net.len();
    let columns = net.first().map_or(0, Vec::len);
    let patch = ssi::Patch {
        u_degree: p,
        v_degree: q,
        u_count: rows,
        v_count: columns,
        poles: net.iter().flatten().copied().collect(),
        weights: Some(weights.iter().flatten().copied().collect()),
        u_knots: u_knots.to_vec(),
        v_knots: v_knots.to_vec(),
        analytic: None,
    };
    let mapping = if spline.v_reversed() {
        let ((_, _), (v0, v1)) = spline.domain();
        Mapping::Reversed { sum: v0 + v1 }
    } else {
        Mapping::Same
    };
    Ok((patch, mapping))
}

/// The surface's own parameters at the engine's `(u, v)` on its patch. An
/// analytic surface is inverted at the patch's point, exactly, and its angles
/// taken round into the box; at a sphere's pole, where no longitude is the
/// right one, the knot reading stands.
fn exact(piece: &Bounded, patch: &ssi::Patch, mapping: Mapping, knot: [f64; 2]) -> [f64; 2] {
    let approximate = mapping.back(knot);
    if matches!(piece.surface, Surface::Plane(_) | Surface::Nurbs(_) | Surface::Offset(_)) {
        return approximate;
    }
    let Some((u, v)) = patch
        .evaluate(knot[0], knot[1])
        .and_then(|point| piece.surface.parameters_at(point))
    else {
        return approximate;
    };
    let turn = std::f64::consts::TAU;
    // The turn of `t` nearest the reading the knot gave.
    let near = |t: f64, guess: f64| t + ((guess - t) / turn).round() * turn;
    let v = match piece.surface {
        Surface::Torus(_) => near(v, approximate[1]),
        _ => v,
    };
    [near(u, approximate[0]), v]
}

/// Whether surface parameters lie in a bounded piece's box, angles taken
/// round a full turn.
fn within(piece: &Bounded, [u, v]: [f64; 2], slack: f64) -> bool {
    let turn = std::f64::consts::TAU;
    let periodic_u = matches!(
        piece.surface,
        Surface::Cylinder(_) | Surface::Cone(_) | Surface::Sphere(_) | Surface::Torus(_)
    );
    let periodic_v = matches!(piece.surface, Surface::Torus(_));
    let inside = |t: f64, [lo, hi]: [f64; 2], periodic: bool| {
        if periodic && hi - lo >= turn - 1e-12 {
            return true;
        }
        if periodic {
            let along = (t - lo).rem_euclid(turn);
            return along <= hi - lo + slack || along >= turn - slack;
        }
        t >= lo - slack && t <= hi + slack
    };
    inside(u, piece.u, periodic_u) && inside(v, piece.v, periodic_v)
}

/// Where two bounded surfaces meet.
///
/// `feature` is the smallest length the answer resolves: a loop or a gap
/// below it may be taken as a point or a touch. A thousandth of the model's
/// size is a sound default.
pub fn meet(first: &Bounded, second: &Bounded, feature: f64) -> Result<Meeting, Refusal> {
    let (patch_a, map_a) = patch(first)?;
    let (patch_b, map_b) = patch(second)?;
    let options = ssi::shapes::resolving(feature, &[&patch_a, &patch_b]);
    let (body_a, body_b) = (std::slice::from_ref(&patch_a), std::slice::from_ref(&patch_b));
    let mut found = ssi::intersect_bodies(body_a, body_b, &options).map_err(|error| match error {
        ssi::Error::Tangent { .. } => Refusal::Tangent(error.to_string()),
        ssi::Error::BadPatch(_) => Refusal::Unsupported(error.to_string()),
        _ => Refusal::Failed(error.to_string()),
    })?;
    ssi::densify(&mut found, body_a, body_b, 0.01).map_err(|error| Refusal::Failed(error.to_string()))?;

    let shared = |q: &ssi::CurvePoint| Shared {
        point: q.p,
        first: exact(first, &patch_a, map_a, [q.a.u, q.a.v]),
        second: exact(second, &patch_b, map_b, [q.b.u, q.b.v]),
    };
    // A patch covers its box exactly, except a spline taken whole for a part
    // of it, and a cone always taken round its whole turn: keep what lies in
    // both boxes.
    let slack = 1e-9;
    let kept = |s: &Shared| within(first, s.first, slack) && within(second, s.second, slack);
    let mut branches = Vec::new();
    for curve in &found.curves {
        let points: Vec<Shared> = curve.points.iter().map(shared).collect();
        if points.iter().all(kept) {
            branches.push(Branch { points, closed: curve.closed });
            continue;
        }
        // Cut at the box: runs of kept points, a run wrapping round the end
        // of a closed curve joined.
        let mut runs: Vec<Vec<Shared>> = Vec::new();
        let mut run = Vec::new();
        for point in points.iter() {
            if kept(point) {
                run.push(*point);
            } else if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
        if curve.closed && runs.len() > 1 && kept(&points[0]) && points.last().is_some_and(kept) {
            let head = runs.remove(0);
            if let Some(last) = runs.last_mut() {
                last.extend(head);
            }
        }
        branches.extend(
            runs.into_iter()
                .filter(|run| run.len() >= 2)
                .map(|points| Branch { points, closed: false }),
        );
    }
    let touches = found.points.iter().map(shared).filter(kept).collect();
    Ok(Meeting { branches, touches, crossings: found.vertices.clone() })
}

/// Whether two bounded surfaces are proved more than `gap` apart, without
/// meshing either.
///
/// Each is cut into its rational Bézier pieces, which lie in the hull of
/// their control points (positive weights). Pairs of pieces are taken
/// nearest first: a pair whose boxes, or a slab across either piece's normal
/// cone, keep them `gap` apart is dropped; any other has its larger piece
/// halved. `true` once every pair is dropped. `false` — nothing proved, not
/// a meeting found — when a pair stays close after its pieces have shrunk to
/// a thousandth of where they started, or after a few thousand pairs.
///
/// Ported from Solidcore's certified face distance bound
/// (`query/distance_bound.rs`, `query/contact.rs`: hull bounds tightened by
/// de Casteljau subdivision, nearest pair first), on the vendored engine's
/// pieces. Apache-2.0, see THIRD_PARTY.md.
pub fn apart(first: &Bounded, second: &Bounded, gap: f64) -> bool {
    use ssi::piece::{pieces_apart, Piece};
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;
    const PAIRS: usize = 4096;
    struct Pair<'p> {
        gap: f64,
        one: &'p Piece,
        other: &'p Piece,
    }
    impl PartialEq for Pair<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.gap.total_cmp(&other.gap) == Ordering::Equal
        }
    }
    impl Eq for Pair<'_> {}
    impl PartialOrd for Pair<'_> {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Pair<'_> {
        // The heap pops the nearest pair first.
        fn cmp(&self, other: &Self) -> Ordering {
            other.gap.total_cmp(&self.gap)
        }
    }
    let box_gap = |one: &Piece, other: &Piece| {
        let (a, b) = (one.bounds(), other.bounds());
        (0..3)
            .map(|axis| (a[axis] - b[axis + 3]).max(b[axis] - a[axis + 3]).max(0.0))
            .map(|apart| apart * apart)
            .sum::<f64>()
            .sqrt()
    };
    let (Ok((one, _)), Ok((other, _))) = (patch(first), patch(second)) else {
        return false;
    };
    let (Ok(ones), Ok(others)) = (one.bezier_pieces(0), other.bezier_pieces(1)) else {
        return false;
    };
    let smallest = ones.iter().chain(&others).map(Piece::size).fold(0.0, f64::max) * 1e-3;
    let mut heap = BinaryHeap::new();
    for one in &ones {
        for other in &others {
            heap.push(Pair { gap: box_gap(one, other), one, other });
        }
    }
    let mut tried = 0;
    while let Some(Pair { one, other, .. }) = heap.pop() {
        tried += 1;
        if tried > PAIRS {
            return false;
        }
        if pieces_apart(one, other, gap) {
            continue;
        }
        if one.size().max(other.size()) <= smallest.max(gap) {
            return false;
        }
        let (larger, kept, first_larger) = match one.size() >= other.size() {
            true => (one, other, true),
            false => (other, one, false),
        };
        let along = if larger.extent(0) >= larger.extent(1) { 0 } else { 1 };
        for half in larger.halves(along) {
            let (one, other) = if first_larger { (half, kept) } else { (kept, half) };
            heap.push(Pair { gap: box_gap(one, other), one, other });
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Cylinder, Sphere};
    use ocs_geom::space::Plane;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn ball(centre: [f64; 3], radius: f64) -> Surface {
        let frame = Plane::from_axes(centre, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        Surface::Sphere(Sphere { frame, radius })
    }

    #[test]
    fn faces_a_gap_apart_are_proved_apart_and_touching_ones_are_not() {
        let whole = |surface| Bounded { surface, u: [0.0, TAU], v: [-FRAC_PI_2, FRAC_PI_2] };
        let one = ball([0.0; 3], 1.0);
        // Two unit balls 2.01 apart: a hundredth between them.
        let near = ball([2.01, 0.0, 0.0], 1.0);
        assert!(apart(&whole(&one), &whole(&near), 1e-3));
        assert!(!apart(&whole(&one), &whole(&near), 0.02));
        // Overlapping, they meet.
        let into = ball([1.5, 0.0, 0.0], 1.0);
        assert!(!apart(&whole(&one), &whole(&into), 1e-6));
        // A rod of radius 0.2 standing 0.25 off the ball's side, and the
        // ball's half turned away from it, which the rod's box overlaps.
        let rod = Surface::Cylinder(Cylinder {
            base: Plane::from_axes([1.45, 0.0, -2.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            radius: 0.2,
        });
        let rod = Bounded { surface: &rod, u: [0.0, TAU], v: [0.0, 4.0] };
        assert!(apart(&whole(&one), &rod, 1e-6));
        let far_half = Bounded { surface: &one, u: [FRAC_PI_2, 3.0 * FRAC_PI_2], v: [-1.0, 1.0] };
        assert!(apart(&far_half, &rod, 1e-6));
        let near_half = Bounded { surface: &one, u: [-PI / 4.0, PI / 4.0], v: [-1.0, 1.0] };
        assert!(!apart(&near_half, &rod, 0.3));
    }
}
