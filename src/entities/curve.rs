//! An entity's geometry as a kernel curve.
//!
//! One converter, so that what is drawn, what is snapped to and what is
//! trimmed all read the same definition. Before this each of those grew its
//! own per-type match, and the three drifted: an arc's tessellation, its snap
//! points and its trim parameters were computed by different code from the
//! same fields.
//!
//! # The plane
//!
//! A [`Curve`] is two-dimensional; a drawing's curve lives on a plane in
//! space. The two together are a [`PlanarCurve`], and the plane comes from
//! the entity's **OCS** — its extrusion normal, run through the DXF
//! arbitrary-axis algorithm in [`ocs_axes`]. Not from the UCS: the UCS is the
//! working plane a user drew in, and an entity does not remember it. Draw a
//! circle in a UCS turned 30° about Z and the stored normal is still +Z, so
//! the frame this builds is the world one — which is exactly right, because
//! that is where the circle's stored coordinates are read.
//!
//! Entities that store WCS coordinates rather than OCS ones (LINE, ELLIPSE,
//! SPLINE, RAY, XLINE) still get a plane, derived from the geometry itself.
//! Where they turn out not to be planar at all, the answer is `None` rather
//! than a silent projection onto XY, which would move the geometry.
//!
//! # What is deliberately absent
//!
//! Text, hatch, dimensions, meshes, solids, images and blocks are not
//! curves. They keep their own paths and this returns `None` for them, which
//! is what lets a caller ask about any entity without checking first.

use acadrust::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, LwPolyline as LwPolylineEnt,
    Polyline2D, Spline as SplineEnt,
};
use acadrust::kernel::geom2d::{
    characteristic_points, Arc, Circle, Curve, Ellipse, EllipseArc, Line, Polyline, PolylineVertex,
    Ray, SnapKind, Transform, XLine,
};
use acadrust::kernel::space::{PlanarCurve, Plane, Vec3};
use acadrust::types::Vector3;
use acadrust::EntityType;

use crate::modules::draw::modify::spline_ops::spline_to_nurbs_on;
use crate::scene::model::wire_model::SnapHint;
use crate::scene::view::transform::ocs_axes;

/// The codec's vector as the array the kernel speaks in. The two have no
/// conversion between them by design — neither crate knows the other — so the
/// bridge is here, once.
fn xyz(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// How far a point may sit off a candidate plane and still be taken to lie on
/// it.
///
/// Relative to the geometry's own size rather than absolute: a spline drawn
/// at survey coordinates carries several orders more rounding in its stored
/// values than one drawn near the origin, and a fixed floor would reject the
/// first or accept anything for the second.
const PLANARITY_TOLERANCE: f64 = 1e-9;

/// The entity's geometry as a curve on the plane it lives on.
///
/// `None` for entities that are not curves, and for the ones that are but
/// whose points do not lie on a plane — a 3D polyline, a spline through
/// points in space.
pub fn entity_curve(entity: &EntityType) -> Option<PlanarCurve> {
    match entity {
        EntityType::Line(line) => straight_curve(line.start, line.end, Straight::Segment),
        EntityType::Ray(ray) => {
            straight_curve(ray.base_point, ray.base_point + ray.direction, Straight::Ray)
        }
        EntityType::XLine(line) => straight_curve(
            line.base_point,
            line.base_point + line.direction,
            Straight::Infinite,
        ),
        EntityType::Circle(circle) => Some(circle_curve(circle)),
        EntityType::Arc(arc) => Some(arc_curve(arc)),
        EntityType::Ellipse(ellipse) => ellipse_curve(ellipse),
        EntityType::LwPolyline(polyline) => lwpolyline_curve(polyline),
        EntityType::Polyline2D(polyline) => polyline2d_curve(polyline),
        EntityType::Spline(spline) => spline_curve(spline),
        _ => None,
    }
}

/// The plane an OCS-stored entity's coordinates are read in.
///
/// `elevation` is the entity's third stored coordinate — the distance along
/// the normal its plane sits at — so the plane's origin is where a stored
/// `(0, 0)` actually lands.
///
/// The axes come from [`ocs_axes`] rather than being derived here. That
/// algorithm is a DXF storage convention with its own history of getting the
/// degenerate cases wrong, and having it in one place is what keeps the
/// second copy from drifting.
pub fn ocs_plane(normal: Vector3, elevation: f64) -> Plane {
    let normal = normalized(normal);
    if is_default_normal(normal) {
        // The overwhelming majority. Named rather than left to the general
        // path so the axes come out bit-exact, which is what lets consumers
        // take the world-XY shortcut.
        return Plane::from_axes([0.0, 0.0, elevation], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    }
    let (x_axis, y_axis) = ocs_axes((normal.x, normal.y, normal.z));
    Plane::from_axes(
        (Vec3::from(xyz(normal)) * elevation).to_array(),
        [x_axis.0, x_axis.1, x_axis.2],
        [y_axis.0, y_axis.1, y_axis.2],
    )
}

/// Which of the three straight kinds a pair of points describes.
enum Straight {
    Segment,
    Ray,
    Infinite,
}

/// A LINE, RAY or XLINE, which store world coordinates and may run anywhere.
///
/// A straight curve lies in infinitely many planes, so any of them gives the
/// same points back. The one picked is the world XY plane when the geometry
/// is level, because that is the frame everything else in a drawing shares
/// and a caller that wants to intersect two curves needs them in one frame.
/// Otherwise it is the upright plane through the line, which is the only
/// choice that does not depend on an arbitrary rotation.
fn straight_curve(from: Vector3, to: Vector3, kind: Straight) -> Option<PlanarCurve> {
    let along = Vec3::from(xyz(to)) - Vec3::from(xyz(from));
    if along.length_squared() <= 0.0 {
        return None;
    }
    let plane = if (to.z - from.z).abs() <= PLANARITY_TOLERANCE * scale_of(&[from, to]) {
        Plane::from_axes([0.0, 0.0, from.z], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    } else {
        // The upright plane through the line: its normal is horizontal, and
        // perpendicular to the line. A line that is itself vertical has no
        // such normal, and any plane containing it will do — XZ is taken.
        let normal = along.cross(Vec3::Z).normalize().unwrap_or(Vec3::Y);
        Plane::orthonormal(xyz(from), along.to_array(), normal.to_array())?
    };
    let start = plane.project(xyz(from))?;
    let end = plane.project(xyz(to))?;
    let direction = [end[0] - start[0], end[1] - start[1]];
    Some(PlanarCurve::new(
        plane,
        match kind {
            Straight::Segment => Curve::Line(Line { start, end }),
            Straight::Ray => Curve::Ray(Ray {
                origin: start,
                direction,
            }),
            Straight::Infinite => Curve::XLine(XLine {
                base: start,
                direction,
            }),
        },
    ))
}

pub fn circle_curve(circle: &CircleEnt) -> PlanarCurve {
    PlanarCurve::new(
        ocs_plane(circle.normal, circle.center.z),
        Curve::Circle(Circle {
            centre: [circle.center.x, circle.center.y],
            radius: circle.radius,
        }),
    )
}

pub fn arc_curve(arc: &ArcEnt) -> PlanarCurve {
    PlanarCurve::new(
        ocs_plane(arc.normal, arc.center.z),
        Curve::Arc(Arc {
            centre: [arc.center.x, arc.center.y],
            radius: arc.radius,
            start_angle: arc.start_angle,
            end_angle: arc.end_angle,
        }),
    )
}

/// An ELLIPSE, whose centre and major axis are stored in world coordinates
/// even though the shape is planar.
///
/// The plane is still the entity's OCS one, so an ellipse and a circle with
/// the same normal end up in the same frame and can be intersected without
/// re-expressing either. Getting there means projecting the two stored
/// vectors rather than reading them off, which is where the major axis picks
/// up its direction *and* its length.
pub fn ellipse_curve(ellipse: &EllipseEnt) -> Option<PlanarCurve> {
    let normal = normalized(ellipse.normal);
    let elevation = Vec3::from(xyz(ellipse.center)).dot(Vec3::from(xyz(normal)));
    let plane = ocs_plane(normal, elevation);
    let centre = plane.project(xyz(ellipse.center))?;
    let major = plane.project_vector(xyz(ellipse.major_axis))?;
    let major_radius = (major[0] * major[0] + major[1] * major[1]).sqrt();
    if major_radius <= 0.0 {
        return None;
    }
    Some(PlanarCurve::new(
        plane,
        Curve::Ellipse(EllipseArc {
            ellipse: Ellipse {
                centre,
                major_radius,
                minor_radius: major_radius * ellipse.minor_axis_ratio,
                major_axis: [major[0] / major_radius, major[1] / major_radius],
            },
            start_parameter: ellipse.start_parameter,
            end_parameter: ellipse.end_parameter,
        }),
    ))
}

pub fn lwpolyline_curve(polyline: &LwPolylineEnt) -> Option<PlanarCurve> {
    let vertices: Vec<PolylineVertex> = polyline
        .vertices
        .iter()
        .map(|v| PolylineVertex {
            position: [v.location.x, v.location.y],
            bulge: v.bulge,
        })
        .collect();
    (vertices.len() >= 2).then(|| {
        PlanarCurve::new(
            ocs_plane(polyline.normal, polyline.elevation),
            Curve::Polyline(Polyline {
                vertices,
                closed: polyline.is_closed,
            }),
        )
    })
}

/// A heavy 2D POLYLINE. Its vertices carry a Z, but the entity's own
/// `elevation` is what defines the plane; the two agree in a well-formed
/// drawing and the entity field is the one the writer round-trips.
pub fn polyline2d_curve(polyline: &Polyline2D) -> Option<PlanarCurve> {
    let vertices: Vec<PolylineVertex> = polyline
        .vertices
        .iter()
        .map(|v| PolylineVertex {
            position: [v.location.x, v.location.y],
            bulge: v.bulge,
        })
        .collect();
    (vertices.len() >= 2).then(|| {
        PlanarCurve::new(
            ocs_plane(polyline.normal, polyline.elevation),
            Curve::Polyline(Polyline {
                vertices,
                closed: polyline.flags.is_closed(),
            }),
        )
    })
}

/// A SPLINE, whose control and fit points are world coordinates that need not
/// lie on any plane.
///
/// The `planar` flag is not trusted on its own — it is routinely wrong in
/// files written by other software — so the points are checked against the
/// plane the normal describes. A spline that genuinely wanders in space gets
/// `None`, which is honest: flattening it to XY would move it.
pub fn spline_curve(spline: &SplineEnt) -> Option<PlanarCurve> {
    let points: Vec<Vector3> = spline
        .control_points
        .iter()
        .chain(spline.fit_points.iter())
        .copied()
        .collect();
    let first = points.first()?;
    let normal = normalized(spline.normal);
    let elevation = Vec3::from(xyz(*first)).dot(Vec3::from(xyz(normal)));
    let plane = ocs_plane(normal, elevation);

    let tolerance = PLANARITY_TOLERANCE * scale_of(&points);
    if !points.iter().all(|p| plane.contains(xyz(*p), tolerance)) {
        return None;
    }
    Some(PlanarCurve::new(
        plane,
        Curve::Nurbs(spline_to_nurbs_on(spline, &plane)?),
    ))
}

/// The entity's curve in world XY coordinates.
///
/// The editing commands — TRIM, EXTEND, FILLET, OFFSET — work in plan view,
/// where every boundary has to be expressed in one shared frame before two of
/// them can be intersected. [`entity_curve`] gives each entity its own plane,
/// which is right for describing the geometry and wrong for comparing two
/// pieces of it; this folds the plane away.
///
/// `None` when the entity is not a planar curve, and when its plane stands
/// edge-on to the view: a circle seen exactly from the side is a line
/// segment, and there is no honest curve to trim against.
///
/// This is also where an extruded entity stops being read wrong. Reading an
/// arc's `center.x/.y` straight out of the entity treats OCS coordinates as
/// world ones, so an arc carrying the `(0, 0, −1)` normal a MIRROR leaves
/// behind acted as a boundary on the wrong side of the drawing.
pub fn entity_curve_xy(entity: &EntityType) -> Option<Curve> {
    let planar = entity_curve(entity)?;
    let plane = planar.plane;
    if plane.is_xy_aligned() {
        // Already world-aligned, so only the elevation has to be dropped and
        // any origin offset carried over.
        let [x, y, _] = plane.origin;
        return if x == 0.0 && y == 0.0 {
            Some(planar.curve)
        } else {
            planar.curve.transformed(&Transform::translation([x, y]))
        };
    }
    // A plane whose normal has no Z component is edge-on: it projects to a
    // line, and a curve on it has no plan-view shape.
    let normal = plane.normal()?;
    if normal[2].abs() <= 1e-12 {
        return None;
    }
    // Otherwise the drop to XY is affine — each plane axis contributes its own
    // X and Y — so the curve can follow it exactly rather than being sampled.
    planar.curve.transformed(&Transform {
        x_axis: [plane.x_axis[0], plane.x_axis[1]].into(),
        y_axis: [plane.y_axis[0], plane.y_axis[1]].into(),
        origin: [plane.origin[0], plane.origin[1]].into(),
    })
}

/// World-space wire points for a curve, cut to the render pass's current
/// chord tolerance.
///
/// The density comes from how far a chord may sag from the curve, not from a
/// segment count: an arc a metre across and one a kilometre across need
/// different numbers of points to look equally round, and the tolerance the
/// render pass carries is already zoom-adaptive.
///
/// Everything stays `f64` to the end. The narrowing belongs at the GPU
/// boundary, where the residual is kept as the low half of a double-single
/// pair; casting local coordinates on the way in throws away precision the
/// shader was built to reconstruct.
pub fn curve_points(curve: &PlanarCurve) -> Vec<[f64; 3]> {
    let tolerance = crate::scene::convert::curve_tol::curve_tol_for(curve_size(&curve.curve));
    curve.tessellate_within(tolerance)
}

/// How big a curve is, for deciding how finely to sample it.
///
/// A radius where there is one, since that is what the chord-height relation
/// is written in terms of. `0` for the kinds whose sampling density is not a
/// question of curvature — a straight run needs its two ends and nothing in
/// between — which leaves those on the frame's own tolerance.
fn curve_size(curve: &Curve) -> f64 {
    match curve {
        Curve::Circle(circle) => circle.radius,
        Curve::Arc(arc) => arc.radius,
        Curve::Ellipse(arc) => arc.ellipse.major_radius,
        // A polyline's bulges and a spline's bends are the same question asked
        // of a chain, and its own extent is the nearest thing to a radius.
        Curve::Polyline(_) | Curve::Nurbs(_) => {
            let sampled = curve.tessellate(64.0);
            let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
            for point in &sampled {
                for axis in 0..2 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            if low[0] > high[0] {
                0.0
            } else {
                (high[0] - low[0]).max(high[1] - low[1]) * 0.5
            }
        }
        Curve::Line(_) | Curve::Ray(_) | Curve::XLine(_) => 0.0,
    }
}

/// The snap candidates an entity's curve offers, in the two channels the
/// wire model carries them in.
#[derive(Debug, Default, Clone)]
pub struct CurveSnap {
    /// Centres, quadrants and midpoints, in world coordinates.
    pub snap_pts: Vec<(glam::DVec3, SnapHint)>,
    /// The ends of the curve and of each of a chain's segments.
    pub key_vertices: Vec<[f64; 3]>,
}

/// Every point the entity's own geometry offers to snap to.
///
/// The point of routing this through [`entity_curve`] rather than reading the
/// fields per type: an arc's quadrants, an ellipse's axis ends and a spline's
/// midpoint are all the same question asked of different shapes, and asking
/// it once means the answer is exact everywhere. Previously an arc offered no
/// quadrants at all and a spline's "endpoints" were its control points, which
/// for a control-point spline are not on the curve.
///
/// `None` for anything that is not a planar curve, which keeps the callers
/// that have their own snap sources — text, blocks, dimensions — untouched.
pub fn curve_snap(entity: &EntityType) -> Option<CurveSnap> {
    Some(snap_from(&entity_curve(entity)?))
}

/// [`curve_snap`] for a caller that already has the curve.
///
/// The per-type wire builders take a concrete entity rather than an
/// [`EntityType`], and wrapping one back up would mean cloning it on the
/// render path.
pub fn snap_from(curve: &PlanarCurve) -> CurveSnap {
    // A chain of straight segments is what `key_vertices` means: the snap
    // engine joins consecutive entries and offers the midpoint of each. Only
    // a line or a polyline can promise that, so those two put their ends
    // there and let their midpoints be derived; everything else names its own
    // ends and middle explicitly.
    //
    // The same distinction is why a polyline's arc-segment centres are not
    // emitted: a wire carrying a centre is treated as round elsewhere, which
    // a polyline with one bulge in it is not.
    let chain = matches!(curve.curve, Curve::Line(_) | Curve::Polyline(_));
    let mut out = CurveSnap::default();
    let mut push = |world: [f64; 3], hint: SnapHint| {
        out.snap_pts.push((glam::DVec3::from_array(world), hint));
    };
    for point in characteristic_points(&curve.curve) {
        let world = curve.plane.point_at(point.point);
        match point.kind {
            SnapKind::Endpoint if chain => out.key_vertices.push(world),
            SnapKind::Endpoint => push(world, SnapHint::Endpoint),
            SnapKind::Midpoint if !chain => push(world, SnapHint::Midpoint),
            SnapKind::Centre if !chain => push(world, SnapHint::Center),
            SnapKind::Quadrant => push(world, SnapHint::Quadrant),
            _ => {}
        }
    }
    out
}

/// The magnitude the geometry's own coordinates sit at, for scaling a
/// relative tolerance. Floored at one so geometry near the origin does not
/// end up with a tolerance of nothing.
fn scale_of(points: &[Vector3]) -> f64 {
    points
        .iter()
        .map(|p| p.x.abs().max(p.y.abs()).max(p.z.abs()))
        .fold(1.0, f64::max)
}

fn is_default_normal(normal: Vector3) -> bool {
    normal.x == 0.0 && normal.y == 0.0 && normal.z == 1.0
}

/// A unit normal, falling back to +Z for the degenerate vector some files
/// store. Zero would make every axis collapse and put the whole entity at
/// one point.
fn normalized(normal: Vector3) -> Vector3 {
    match Vec3::from(xyz(normal)).normalize() {
        Some(unit) => Vector3::new(unit.x, unit.y, unit.z),
        None => Vector3::new(0.0, 0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::lwpolyline::LwVertex;
    use acadrust::entities::{Line as LineEnt, Ray as RayEnt, XLine as XLineEnt};
    use acadrust::types::Vector2;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn v3(x: f64, y: f64, z: f64) -> Vector3 {
        Vector3::new(x, y, z)
    }

    /// Every point the curve reports, against a direct evaluation of the
    /// entity's own fields. The property that matters: the converter must not
    /// move anything.
    fn assert_on(curve: &PlanarCurve, expected: impl Fn(f64) -> [f64; 3]) {
        for i in 0..=8 {
            let t = i as f64 / 8.0;
            let got = curve.point_at(t);
            let want = expected(t);
            for axis in 0..3 {
                assert!(
                    (got[axis] - want[axis]).abs() < 1e-9,
                    "t={t}: {got:?} vs {want:?}"
                );
            }
        }
    }

    #[test]
    fn a_flat_circle_lands_on_the_world_xy_plane() {
        let mut circle = CircleEnt::default();
        circle.center = v3(10.0, 20.0, 4.0);
        circle.radius = 3.0;
        circle.normal = v3(0.0, 0.0, 1.0);
        let curve = entity_curve(&EntityType::Circle(circle)).unwrap();
        assert!(curve.plane.is_xy_aligned());
        assert_on(&curve, |t| {
            let a = t * TAU;
            [10.0 + 3.0 * a.cos(), 20.0 + 3.0 * a.sin(), 4.0]
        });
    }

    #[test]
    fn an_extruded_arc_uses_the_arbitrary_axis_frame() {
        // Normal along −Z: the frame flips, and a converter that ignored it
        // would put the arc on the wrong side of the drawing.
        let mut arc = ArcEnt::default();
        arc.center = v3(5.0, 0.0, 2.0);
        arc.radius = 1.0;
        arc.start_angle = 0.0;
        arc.end_angle = FRAC_PI_2;
        arc.normal = v3(0.0, 0.0, -1.0);
        let curve = entity_curve(&EntityType::Arc(arc)).unwrap();
        assert!(!curve.plane.is_xy_aligned());
        assert_on(&curve, |t| {
            let a = t * FRAC_PI_2;
            let wcs = crate::scene::view::transform::ocs_point_to_wcs(
                (5.0 + a.cos(), a.sin(), 2.0),
                (0.0, 0.0, -1.0),
            );
            [wcs.0, wcs.1, wcs.2]
        });
    }

    #[test]
    fn a_level_line_shares_the_world_frame() {
        let mut line = LineEnt::default();
        line.start = v3(1.0, 2.0, 7.0);
        line.end = v3(4.0, 6.0, 7.0);
        let curve = entity_curve(&EntityType::Line(line)).unwrap();
        assert!(curve.plane.is_xy_aligned(), "{:?}", curve.plane);
        assert_on(&curve, |t| [1.0 + 3.0 * t, 2.0 + 4.0 * t, 7.0]);
    }

    #[test]
    fn a_sloping_line_still_reports_its_own_points() {
        // No XY frame contains it, so the plane is the upright one through
        // the line. The points are what must not change.
        let mut line = LineEnt::default();
        line.start = v3(0.0, 0.0, 0.0);
        line.end = v3(3.0, 4.0, 12.0);
        let curve = entity_curve(&EntityType::Line(line)).unwrap();
        assert!(!curve.plane.is_xy_aligned());
        assert_on(&curve, |t| [3.0 * t, 4.0 * t, 12.0 * t]);
    }

    #[test]
    fn a_vertical_line_is_not_a_degenerate_case() {
        // `along × Z` is zero here, which is where an unguarded normal would
        // collapse the frame and send every point to the origin.
        let mut line = LineEnt::default();
        line.start = v3(2.0, 3.0, 0.0);
        line.end = v3(2.0, 3.0, 10.0);
        let curve = entity_curve(&EntityType::Line(line)).unwrap();
        assert_on(&curve, |t| [2.0, 3.0, 10.0 * t]);
    }

    #[test]
    fn a_zero_length_line_has_no_curve() {
        let mut line = LineEnt::default();
        line.start = v3(1.0, 1.0, 1.0);
        line.end = v3(1.0, 1.0, 1.0);
        assert!(entity_curve(&EntityType::Line(line)).is_none());
    }

    #[test]
    fn an_ellipse_keeps_its_axis_length_and_direction() {
        let mut ellipse = EllipseEnt::default();
        ellipse.center = v3(10.0, 5.0, 0.0);
        ellipse.major_axis = v3(0.0, 4.0, 0.0); // up, length 4
        ellipse.minor_axis_ratio = 0.5;
        ellipse.start_parameter = 0.0;
        ellipse.end_parameter = TAU;
        ellipse.normal = v3(0.0, 0.0, 1.0);
        let curve = entity_curve(&EntityType::Ellipse(ellipse)).unwrap();
        let Curve::Ellipse(arc) = &curve.curve else {
            panic!("expected an ellipse");
        };
        assert!((arc.ellipse.major_radius - 4.0).abs() < 1e-12);
        assert!((arc.ellipse.minor_radius - 2.0).abs() < 1e-12);
        assert_on(&curve, |t| {
            let a = t * TAU;
            // Major axis points along +Y, so the roles of the two axes swap.
            [10.0 - 2.0 * a.sin(), 5.0 + 4.0 * a.cos(), 0.0]
        });
    }

    #[test]
    fn an_ellipse_on_an_extruded_plane_stays_on_it() {
        let mut ellipse = EllipseEnt::default();
        ellipse.center = v3(0.0, 0.0, 3.0);
        ellipse.major_axis = v3(2.0, 0.0, 0.0);
        ellipse.minor_axis_ratio = 0.5;
        ellipse.start_parameter = 0.0;
        ellipse.end_parameter = PI;
        ellipse.normal = v3(0.0, 1.0, 0.0);
        // The centre is off that plane, so the entity is inconsistent and the
        // projection is what decides. What must hold is that the result is a
        // planar curve whose points all sit on the plane it reports.
        let curve = entity_curve(&EntityType::Ellipse(ellipse)).unwrap();
        for point in curve.tessellate(20.0) {
            assert!(curve.plane.contains(point, 1e-9), "{point:?}");
        }
    }

    #[test]
    fn a_polyline_carries_its_bulges_and_closure() {
        let mut polyline = LwPolylineEnt::default();
        polyline.elevation = 2.0;
        polyline.is_closed = true;
        polyline.normal = v3(0.0, 0.0, 1.0);
        polyline.vertices = vec![
            LwVertex::with_bulge(Vector2::new(0.0, 0.0), 1.0),
            LwVertex::from_coords(10.0, 0.0),
        ];
        let curve = entity_curve(&EntityType::LwPolyline(polyline)).unwrap();
        assert!(curve.is_closed());
        // A bulge of 1 is a half circle. Left to right it dips below.
        let points = curve.tessellate(20.0);
        assert!(points.iter().all(|p| (p[2] - 2.0).abs() < 1e-12));
        assert!(
            points.iter().any(|p| p[1] < -4.9),
            "the semicircle should reach y = −5"
        );
    }

    #[test]
    fn a_two_point_minimum_is_enforced() {
        let mut polyline = LwPolylineEnt::default();
        polyline.vertices = vec![LwVertex::from_coords(0.0, 0.0)];
        assert!(entity_curve(&EntityType::LwPolyline(polyline)).is_none());
    }

    #[test]
    fn a_flat_spline_converts_and_a_spatial_one_does_not() {
        let mut spline = SplineEnt::default();
        spline.degree = 3;
        spline.fit_points = vec![
            v3(0.0, 0.0, 5.0),
            v3(1.0, 2.0, 5.0),
            v3(3.0, 1.0, 5.0),
            v3(5.0, 4.0, 5.0),
        ];
        spline.normal = v3(0.0, 0.0, 1.0);
        let curve = entity_curve(&EntityType::Spline(spline.clone())).unwrap();
        assert!(curve.plane.is_xy_aligned());
        for point in curve.tessellate(20.0) {
            assert!((point[2] - 5.0).abs() < 1e-9, "{point:?}");
        }

        // One point lifted out of the plane. The `planar` flag still says
        // nothing; the geometry is what is checked.
        spline.fit_points[2].z = 9.0;
        assert!(entity_curve(&EntityType::Spline(spline)).is_none());
    }

    fn hints(snap: &CurveSnap, want: SnapHint) -> Vec<glam::DVec3> {
        snap.snap_pts
            .iter()
            .filter(|(_, hint)| std::mem::discriminant(hint) == std::mem::discriminant(&want))
            .map(|&(p, _)| p)
            .collect()
    }

    #[test]
    fn an_arc_offers_the_quadrants_its_sweep_covers() {
        let mut arc = ArcEnt::default();
        arc.center = v3(0.0, 0.0, 0.0);
        arc.radius = 2.0;
        arc.start_angle = 0.0;
        arc.end_angle = PI; // the upper half
        arc.normal = v3(0.0, 0.0, 1.0);
        let snap = curve_snap(&EntityType::Arc(arc)).unwrap();
        let quadrants = hints(&snap, SnapHint::Quadrant);
        // 0° and 90° and 180° are on it; 270° is not.
        assert_eq!(quadrants.len(), 3, "{quadrants:?}");
        assert!(quadrants.iter().all(|q| q.y >= -1e-9));

        assert_eq!(hints(&snap, SnapHint::Center), vec![glam::DVec3::ZERO]);
        assert_eq!(hints(&snap, SnapHint::Midpoint).len(), 1);
        // Ends go through the hint channel, not `key_vertices`: a pair there
        // would also offer the chord's midpoint, which is not on the arc.
        assert_eq!(hints(&snap, SnapHint::Endpoint).len(), 2);
        assert!(snap.key_vertices.is_empty());
    }

    #[test]
    fn a_polyline_puts_its_vertices_in_the_chain_channel() {
        let mut polyline = LwPolylineEnt::default();
        polyline.normal = v3(0.0, 0.0, 1.0);
        polyline.vertices = vec![
            LwVertex::from_coords(0.0, 0.0),
            LwVertex::from_coords(10.0, 0.0),
            LwVertex::from_coords(10.0, 5.0),
        ];
        let snap = curve_snap(&EntityType::LwPolyline(polyline)).unwrap();
        assert_eq!(snap.key_vertices.len(), 3);
        // Midpoints are derived from those by the snap engine, so emitting
        // them here as well would offer every one of them twice.
        assert!(snap.snap_pts.is_empty(), "{:?}", snap.snap_pts);
    }

    #[test]
    fn a_closed_curve_has_no_ends_to_offer() {
        let mut circle = CircleEnt::default();
        circle.radius = 1.0;
        circle.normal = v3(0.0, 0.0, 1.0);
        let snap = curve_snap(&EntityType::Circle(circle)).unwrap();
        assert!(snap.key_vertices.is_empty());
        assert!(hints(&snap, SnapHint::Endpoint).is_empty());
        assert!(hints(&snap, SnapHint::Midpoint).is_empty());
        assert_eq!(hints(&snap, SnapHint::Quadrant).len(), 4);
    }

    #[test]
    fn a_closed_fit_point_spline_comes_back_closed() {
        // The interpolation behind a fit-point spline is a clamped solve and
        // does not model a wrap, so without closing the point list the curve
        // ended somewhere else entirely — and a TRIM against it cut nothing
        // along the seam.
        let mut spline = SplineEnt::default();
        spline.degree = 3;
        spline.flags.closed = true;
        spline.fit_points = vec![
            v3(0.0, 0.0, 0.0),
            v3(10.0, 0.0, 0.0),
            v3(10.0, 10.0, 0.0),
            v3(0.0, 10.0, 0.0),
        ];
        spline.normal = v3(0.0, 0.0, 1.0);
        let curve = entity_curve(&EntityType::Spline(spline)).unwrap();
        let (start, end) = (curve.point_at(0.0), curve.point_at(1.0));
        assert!(
            (start[0] - end[0]).abs() < 1e-9 && (start[1] - end[1]).abs() < 1e-9,
            "{start:?} vs {end:?}"
        );
        assert!(curve.is_closed());
    }

    /// The whole stack resolves from here: OCS reaches the B-rep layer and
    /// the ACIS bridge through the same alias every other CAD type comes in
    /// by. A compile-time check, so a chain that stops resolving is caught
    /// where it happens rather than the next time somebody reaches for it.
    #[test]
    fn the_solid_layer_and_the_acis_bridge_are_reachable() {
        let solid = acadrust::kernel::brep::make::cuboid([0.0; 3], [1.0; 3])
            .expect("the kernel builds its own primitives");
        assert!(solid.validate().is_empty());
        assert_eq!(solid.euler_characteristic(), 2);
        let document = acadrust::entities::acis::types::SatDocument::new();
        let (bodies, loss) = acadrust::acis::lift(&document);
        assert!(bodies.is_empty() && loss.is_empty(), "an empty document lifts to nothing");
    }

    #[test]
    fn entities_that_are_not_curves_say_so() {
        assert!(entity_curve(&EntityType::Point(Default::default())).is_none());
        assert!(entity_curve(&EntityType::Text(Default::default())).is_none());
        assert!(entity_curve(&EntityType::Hatch(Default::default())).is_none());
        assert!(entity_curve(&EntityType::MText(Default::default())).is_none());
        // A 3D polyline is a curve but not a planar one, so it goes the same
        // way rather than being flattened.
        assert!(entity_curve(&EntityType::Polyline3D(Default::default())).is_none());
    }

    #[test]
    fn a_zero_normal_falls_back_rather_than_collapsing() {
        let mut circle = CircleEnt::default();
        circle.center = v3(1.0, 2.0, 0.0);
        circle.radius = 1.0;
        circle.normal = v3(0.0, 0.0, 0.0);
        let curve = entity_curve(&EntityType::Circle(circle)).unwrap();
        assert!(curve.plane.is_xy_aligned());
        assert_eq!(curve.point_at(0.0), [2.0, 2.0, 0.0]);
    }

    #[test]
    fn a_ray_keeps_its_direction_and_a_construction_line_its_extent() {
        let mut ray = RayEnt::default();
        ray.base_point = v3(1.0, 1.0, 0.0);
        ray.direction = v3(2.0, 0.0, 0.0);
        let curve = entity_curve(&EntityType::Ray(ray)).unwrap();
        assert_eq!(curve.extent(), acadrust::kernel::geom2d::Extent::Forward);
        assert_eq!(curve.point_at(1.0), [3.0, 1.0, 0.0]);

        let mut line = XLineEnt::default();
        line.base_point = v3(0.0, 0.0, 0.0);
        line.direction = v3(0.0, 3.0, 0.0);
        let curve = entity_curve(&EntityType::XLine(line)).unwrap();
        assert_eq!(curve.extent(), acadrust::kernel::geom2d::Extent::Infinite);
        assert_eq!(curve.point_at(-1.0), [0.0, -3.0, 0.0]);
    }
}
