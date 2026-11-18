//! Profile-preserving path sweeps, independent of application entities.
//!
//! Profile curves stay rational curves: a circle never becomes a polygon.
//! Undeformed straight and circular paths use the analytic sweep builders.
//! General paths use tolerance-controlled cubic transport patches, rational
//! in the profile parameter, with shared rims and rails between patches.

use super::geometry::{Circle3, Curve3, Surface};
use super::nurbs_builder::{RationalCurve2, RationalCurve3};
use super::topology::{
    Body, Coedge, CoedgeKey, Edge, EdgeKey, Face, FaceKey, Loop, Lump, Shell, ShellKey, Vertex,
    VertexKey,
};
use super::Provenance;
use super::Placement;
use ocs_geom::geom2d::{Arc, Curve, Line, NurbsCurve, Transform};
use ocs_geom::space::{NurbsCurve3, NurbsSurface3, Plane, Vec3};
use std::f64::consts::{PI, TAU};

/// A connected path, expressed without projecting three-dimensional curves.
#[derive(Clone, Copy)]
pub enum SweepPath<'a> {
    Planar { plane: Plane, curves: &'a [Curve] },
    Polyline3d { points: &'a [[f64; 3]], closed: bool },
    Nurbs3(&'a NurbsCurve3),
}

/// Placement and deformation of a swept profile. Angles are radians.
#[derive(Clone, Copy, Debug)]
pub struct SweepOptions {
    /// Align the profile normal to the initial path tangent.
    pub align: bool,
    /// World-space point on the profile placed at the path start.
    pub base_point: Option<[f64; 3]>,
    /// Initial rotation about the profile normal.
    pub rotation: f64,
    /// Additional rotation accumulated over the path length.
    pub twist: f64,
    /// Final scale; scale changes linearly from one over the path length.
    pub scale: f64,
    /// Follow the path's curvature frame instead of minimal-twist transport.
    pub bank: bool,
    /// Omit caps, including when the supplied profile is closed.
    pub surface: bool,
    /// Axes of the profile's own plane where it was drawn, when the profile
    /// supplied has already been placed on the path. They size the rail a
    /// scaled sweep follows (see `sweep_rail_length`); `None` uses the
    /// supplied profile plane.
    pub profile_axes: Option<[[f64; 3]; 2]>,
}

impl Default for SweepOptions {
    fn default() -> Self {
        Self {
            align: true,
            base_point: None,
            rotation: 0.0,
            twist: 0.0,
            scale: 1.0,
            bank: false,
            surface: false,
            profile_axes: None,
        }
    }
}

/// Curve-length weighted common anchor for several independent profiles.
/// Relative profile positions are preserved when this same world-space
/// point is supplied as `base_point` for every member of a sweep selection.
pub fn sweep_profile_group_base(profiles: &[(Plane, Vec<Vec<Curve>>)]) -> Option<[f64; 3]> {
    let (first_plane, first_wires) = profiles.first()?;
    let origin = Vec3::from(sweep_profile_base(*first_plane, first_wires)?);
    let mut moment = Vec3::ZERO;
    let mut length = 0.0;
    for (plane, wires) in profiles {
        let base = Vec3::from(sweep_profile_base(*plane, wires)?);
        let mut profile_length = 0.0;
        for wire in wires {
            for curve in expanded(wire)? {
                profile_length += Piece::Planar(*plane, curve, true).length();
            }
        }
        moment = moment + (base - origin) * profile_length;
        length += profile_length;
    }
    if !length.is_finite() || length <= 1e-14 || !moment.is_finite() { return None; }
    Some((origin + moment / length).to_array())
}

/// Directed first point of a valid bounded path.
pub fn sweep_path_start(path: SweepPath<'_>) -> Option<[f64; 3]> {
    Some(path_pieces(path)?.first()?.point(0.0).to_array())
}

/// Unit tangent where a sweep path starts.
pub fn sweep_path_tangent(path: SweepPath<'_>) -> Option<[f64; 3]> {
    Some(path_pieces(path)?.first()?.tangent(0.0)?.to_array())
}

/// Rigid placement of the original profile onto the first sweep section.
/// Initial twist and scale are zero and one respectively; accumulated
/// deformation therefore does not affect this source-profile grip mapping.
pub fn sweep_profile_placement(
    plane: Plane, wires: &[Vec<Curve>], path: SweepPath<'_>, options: SweepOptions,
) -> Option<Placement> {
    let pieces = path_pieces(path)?;
    let first = pieces.first()?;
    let base = Vec3::from(options.base_point.or_else(|| sweep_profile_base(plane, wires))?);
    initial_placement(plane, base, first.point(0.0), first.tangent(0.0)?, options)
}

fn initial_placement(plane: Plane, base: Vec3, start: Vec3, tangent: Vec3, options: SweepOptions) -> Option<Placement> {
    if !base.is_finite() || !start.is_finite() || !options.rotation.is_finite() { return None; }
    let normal = Vec3::from(plane.normal()?);
    let source_x = Vec3::from(plane.x_axis).normalize()?;
    let source_y = normal.cross(source_x).normalize()?;
    let projected_x = source_x - tangent * source_x.dot(tangent);
    let (target_x, target_y, target_normal) = if projected_x.length() > 1e-10 {
        let x = projected_x.normalize()?;
        (x, tangent.cross(x).normalize()?, tangent)
    } else {
        // A path parallel to the profile's first axis has no projected
        // first direction. Keep the profile's cyclic frame in this case;
        // reversing that path must not turn the selected profile over.
        (source_y, normal, source_x)
    };
    let axis = if options.align { target_normal } else { normal };
    let map = |vector: Vec3| -> Option<Vec3> {
        let aligned = if options.align {
            target_x * vector.dot(source_x) + target_y * vector.dot(source_y) + target_normal * vector.dot(normal)
        } else { vector };
        Some(rotate(aligned, axis, options.rotation))
    };
    Some(Placement { x_axis: map(Vec3::X)?.to_array(), y_axis: map(Vec3::Y)?.to_array(),
        z_axis: map(Vec3::Z)?.to_array(), origin: (start - map(base)?).to_array() })
}

/// Default source-profile anchor. Closed conics use their centre and open
/// conics their middle point; other open chains use their middle point by
/// length, and other closed chains the mean of twenty equally spaced
/// boundary samples including the directed endpoints.
/// Multiple boundary loops contribute in proportion to their curve lengths.
pub fn sweep_profile_base(plane: Plane, wires: &[Vec<Curve>]) -> Option<[f64; 3]> {
    plane.normal()?;
    let origin = Vec3::from(plane.point_at(wires.first()?.first()?.point_at(0.0)));
    let mut total = 0.0;
    let mut moment = Vec3::ZERO;
    for wire in wires {
        let pieces = expanded(wire)?;
        let senses = chain_senses(&pieces)?;
        let closed = chain_closed(&pieces, &senses);
        let path = pieces.iter().zip(senses).map(|(curve, forward)|
            Piece::Planar(plane, curve.clone(), forward)).collect::<Vec<_>>();
        let lengths = path.iter().map(Piece::length).collect::<Vec<_>>();
        let length = lengths.iter().sum::<f64>();
        if !length.is_finite() || length <= 1e-14 { return None; }
        // The point `distance` along the chain.
        let along = |mut distance: f64| {
            let mut index = 0;
            while index + 1 < path.len() && distance > lengths[index] {
                distance -= lengths[index];
                index += 1;
            }
            let parameter = if matches!(&pieces[index], Curve::Line(_) | Curve::Arc(_)) && plane.is_orthonormal() {
                (distance / lengths[index]).clamp(0.0, 1.0)
            } else {
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..40 {
                    let middle = (low + high) * 0.5;
                    if path[index].length_to(middle) < distance { low = middle; } else { high = middle; }
                }
                (low + high) * 0.5
            };
            path[index].point(parameter)
        };
        let anchor = if let Some(point) = conic_anchor(&pieces) {
            Vec3::from(plane.point_at(point))
        } else if !closed {
            along(length * 0.5)
        } else {
            let mut samples = Vec3::ZERO;
            for sample in 0..20 {
                samples = samples + (along(length * sample as f64 / 19.0) - origin);
            }
            origin + samples / 20.0
        };
        moment = moment + (anchor - origin) * length;
        total += length;
    }
    if !total.is_finite() || total <= 1e-14 || !moment.is_finite() { return None; }
    Some((origin + moment / total).to_array())
}

/// The base point of one profile swept along a path that starts at
/// `start` with direction `tangent`. A path leaving the profile's plane
/// from inside its region or its boundary sweeps the profile where it
/// stands: the start is the base. A start elsewhere, or a path that runs
/// along the profile's plane, uses the profile's own anchor
/// (`sweep_profile_base`).
pub fn sweep_profile_base_from(plane: Plane, wires: &[Vec<Curve>], start: [f64; 3], tangent: [f64; 3]) -> Option<[f64; 3]> {
    let anchor = sweep_profile_base(plane, wires)?;
    let normal = Vec3::from(plane.normal()?);
    if Vec3::from(tangent).normalize()?.dot(normal).abs() <= 1e-9 {
        return Some(anchor);
    }
    let uv = plane.project(start)?;
    let size = Vec3::from(anchor).distance(Vec3::from(start)).max(1.0);
    if Vec3::from(plane.point_at(uv)).distance(Vec3::from(start)) > size * 1e-9 {
        return Some(anchor);
    }
    let tolerance = ocs_geom::geom2d::Tolerance::new(size * 1e-9);
    // Holes are taken out of the region: inside an odd number of loops.
    let inside = wires.iter().filter(|wire| ocs_geom::geom2d::containment::contains(wire, uv, tolerance)).count() % 2 == 1;
    let on_boundary = wires.iter().flatten()
        .any(|curve| ocs_geom::geom2d::containment::distance_to(curve, uv) <= tolerance.linear());
    Some(if inside || on_boundary { start } else { anchor })
}

fn conic_anchor(pieces: &[Curve]) -> Option<[f64; 2]> {
    let center = match pieces.first()? {
        Curve::Arc(first) if pieces.iter().all(|piece| matches!(piece, Curve::Arc(arc)
            if near2(first.centre, arc.centre) && (first.radius - arc.radius).abs() <= first.radius.abs().max(1.0) * 1e-10)) => Some(first.centre),
        Curve::Ellipse(first) if pieces.iter().all(|piece| matches!(piece, Curve::Ellipse(arc)
            if first.ellipse == arc.ellipse)) => Some(first.ellipse.centre),
        _ => None,
    }?;
    let senses = chain_senses(pieces)?;
    if chain_closed(pieces, &senses) { return Some(center); }
    let spans = pieces.iter().map(|piece| match piece {
        Curve::Arc(arc) => arc.sweep(),
        Curve::Ellipse(arc) => arc.sweep(),
        _ => 0.0,
    }).collect::<Vec<_>>();
    let mut middle = spans.iter().sum::<f64>() * 0.5;
    let mut index = 0;
    while index + 1 < spans.len() && middle > spans[index] {
        middle -= spans[index];
        index += 1;
    }
    let parameter = middle / spans[index];
    Some(pieces[index].point_at(if senses[index] { parameter } else { 1.0 - parameter }))
}

/// Why a sweep along a path with a corner is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepRefusal {
    /// The section would scale across a mitred joint.
    Scale,
    /// The section would twist across a mitred joint.
    Twist,
    /// Banking along a planar path with a corner.
    Bank,
}

/// Whether two path tangents meet at a corner rather than run on smoothly.
fn is_corner(before: Vec3, after: Vec3) -> bool {
    before.dot(after) < 1.0 - 1e-9
}

/// Recorded sweeps refuse to twist or scale along a path with a corner (a
/// mitred joint cannot change the section on both sides), and to bank along
/// a planar path with a corner, as the reference modeler does.
pub fn sweep_corner_refusal(path: SweepPath<'_>, options: SweepOptions) -> Option<SweepRefusal> {
    let planar = matches!(path, SweepPath::Planar { .. });
    let pieces = path_pieces(path)?;
    let start = pieces.first()?.point(0.0);
    let end = pieces.last()?.point(1.0);
    let extent = pieces.iter().map(|piece| piece.length()).sum::<f64>();
    corner_refusal(&pieces, start.distance(end) <= extent.max(1.0) * 1e-9, planar, options)?
}

fn corner_refusal(
    pieces: &[Piece],
    closed: bool,
    planar: bool,
    options: SweepOptions,
) -> Option<Option<SweepRefusal>> {
    let joints = pieces.windows(2).map(|pair| (&pair[0], &pair[1]))
        .chain(closed.then(|| (pieces.last().unwrap(), &pieces[0])));
    let mut cornered = false;
    for (before, after) in joints {
        cornered |= is_corner(before.tangent(1.0)?, after.tangent(0.0)?);
    }
    Some(if !cornered {
        None
    } else if (options.scale - 1.0).abs() > 1e-12 {
        Some(SweepRefusal::Scale)
    } else if options.twist.abs() > 1e-12 {
        Some(SweepRefusal::Twist)
    } else if options.bank && planar {
        Some(SweepRefusal::Bank)
    } else {
        None
    })
}

/// Sweeps an outer profile and optional holes along a connected spatial path.
/// A single open wire produces a sheet even when `surface` is false.
///
/// Invalid/zero paths, a reversing cusp, non-positive scale, invalid profile
/// input, and incompatible deformation at a closed seam return `None`.
/// General-path fits are checked against 1e-7 of the local model extent;
/// excessive complexity is refused rather than silently reducing accuracy.
pub fn sweep_path(
    profile_plane: Plane,
    wires: &[Vec<Curve>],
    path: SweepPath<'_>,
    options: SweepOptions,
) -> Option<Body> {
    sweep_path_lifted(profile_plane, wires, None, path, options)
}

/// Whether a sweep path has a corner (including a closed path's closing
/// corner).
pub fn sweep_path_has_corner(path: SweepPath<'_>) -> Option<bool> {
    let pieces = path_pieces(path)?;
    let start = pieces.first()?.point(0.0);
    let end = pieces.last()?.point(1.0);
    let extent = pieces.iter().map(|piece| piece.length()).sum::<f64>();
    let closed = start.distance(end) <= extent.max(1.0) * 1e-9;
    let joints = pieces.windows(2).map(|pair| (&pair[0], &pair[1]))
        .chain(closed.then(|| (pieces.last().unwrap(), &pieces[0])));
    let mut cornered = false;
    for (before, after) in joints {
        cornered |= is_corner(before.tangent(1.0)?, after.tangent(0.0)?);
    }
    Some(cornered)
}

/// The anchor of a spatial polyline profile, by 3D length, the same rule as
/// for planar profiles: an open one's middle point, a closed one's mean of
/// twenty equally spaced samples along it (both ends included).
pub fn sweep_polyline_base(points: &[[f64; 3]], closed: bool) -> Option<[f64; 3]> {
    let mut chain = points.iter().map(|p| Vec3::from(*p)).collect::<Vec<_>>();
    if closed { chain.push(*chain.first()?); }
    let lengths = chain.windows(2).map(|pair| pair[0].distance(pair[1])).collect::<Vec<_>>();
    let total = lengths.iter().sum::<f64>();
    if chain.len() < 2 || !total.is_finite() || total <= 1e-14 { return None; }
    let along = |mut distance: f64| {
        let mut index = 0;
        while index + 1 < lengths.len() && distance > lengths[index] {
            distance -= lengths[index];
            index += 1;
        }
        let t = if lengths[index] > 0.0 { (distance / lengths[index]).clamp(0.0, 1.0) } else { 0.0 };
        chain[index] + (chain[index + 1] - chain[index]) * t
    };
    if !closed {
        return Some(along(total * 0.5).to_array());
    }
    let mut sum = Vec3::ZERO;
    for sample in 0..20 {
        sum = sum + along(total * sample as f64 / 19.0);
    }
    Some((sum * (1.0 / 20.0)).to_array())
}

/// Sweeps a spatial (non-planar) polyline profile as a surface, as the
/// reference modeler does: the profile is kept in the XY plane through its
/// anchor, and the height of each point above that plane is carried along
/// the path tangent (scaled and twisted with the section). A profile whose
/// plan view has a zero-length side cannot be swept.
pub fn sweep_spatial_polyline(points: &[[f64; 3]], closed: bool, path: SweepPath<'_>, mut options: SweepOptions) -> Option<Body> {
    if points.len() < 2 { return None; }
    let count = points.len();
    // A closed profile runs counter-clockwise in plan, so each point keeps
    // its own height when the section is oriented.
    let plan_area = (0..count).map(|index| {
        let (a, b) = (points[index], points[(index + 1) % count]);
        a[0] * b[1] - b[0] * a[1]
    }).sum::<f64>();
    let reordered;
    let points = if closed && plan_area < 0.0 {
        reordered = std::iter::once(points[0]).chain(points[1..].iter().rev().copied()).collect::<Vec<_>>();
        &reordered[..]
    } else {
        points
    };
    // A path that starts on the profile sweeps it where it stands.
    let start = Vec3::from(sweep_path_start(path)?);
    let size = points.iter().map(|p| Vec3::from(*p).distance(start)).fold(1.0_f64, f64::max);
    let on_profile = (0..if closed { count } else { count - 1 }).any(|index| {
        let (a, b) = (Vec3::from(points[index]), Vec3::from(points[(index + 1) % count]));
        let along = b - a;
        let t = ((start - a).dot(along) / along.dot(along).max(1e-300)).clamp(0.0, 1.0);
        (a + along * t).distance(start) <= size * 1e-9
    });
    let base = options.base_point
        .or_else(|| on_profile.then_some(start.to_array()))
        .or_else(|| sweep_polyline_base(points, closed))?;
    let plane = Plane::from_axes([0.0, 0.0, base[2]], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let segments = if closed { count } else { count - 1 };
    let mut wire = Vec::with_capacity(segments);
    for index in 0..segments {
        let (a, b) = (points[index], points[(index + 1) % count]);
        if (b[0] - a[0]).hypot(b[1] - a[1]) <= 1e-9 * a[0].abs().max(a[1].abs()).max(1.0) { return None; }
        wire.push(Curve::Line(Line { start: [a[0], a[1]], end: [b[0], b[1]] }));
    }
    let heights = points.iter().map(|p| p[2] - base[2]).collect::<Vec<_>>();
    options.base_point = Some(base);
    options.surface = true;
    sweep_path_lifted(plane, &[wire], Some(&heights), path, options)
}

fn sweep_path_lifted(
    profile_plane: Plane,
    wires: &[Vec<Curve>],
    lift: Option<&[f64]>,
    path: SweepPath<'_>,
    mut options: SweepOptions,
) -> Option<Body> {
    if !options.rotation.is_finite() || !options.twist.is_finite()
        || !options.scale.is_finite() || options.scale <= 1e-9
        || options.twist.abs() > TAU * 1024.0
    {
        return None;
    }
    let mut wires = prepare_wires(wires)?;
    let sheet = options.surface || !wires[0].closed;
    if !sheet && wires.iter().any(|wire| !wire.closed) {
        return None;
    }
    if wires.iter().any(|wire| !wire.closed) && wires.len() != 1 {
        return None;
    }
    let pieces = path_pieces(path)?;
    let start = pieces.first()?.point(0.0);
    let tangent = pieces.first()?.tangent(0.0)?;
    let end = pieces.last()?.point(1.0);
    let extent = pieces.iter().map(|piece| piece.length()).sum::<f64>();
    if !extent.is_finite() || extent <= 1e-12 {
        return None;
    }
    let closed = start.distance(end) <= extent.max(1.0) * 1e-9;
    let mut source_wires = wires.iter().map(|wire| wire.source.clone()).collect::<Vec<_>>();
    let base = Vec3::from(options.base_point
        .or_else(|| sweep_profile_base(profile_plane, &source_wires))?);
    if !base.is_finite() {
        return None;
    }
    // Keep local coordinates near the anchor. Otherwise a tiny rotation of a
    // profile far from the origin magnifies cancellation in transport fits.
    let uv = profile_plane.project(base.to_array())?;
    let recenter = Transform::translation([-uv[0], -uv[1]]);
    source_wires = source_wires.iter().map(|wire| wire.iter().map(|curve|
        curve.transformed(&recenter)).collect::<Option<Vec<_>>>()).collect::<Option<Vec<_>>>()?;
    wires = prepare_wires(&source_wires)?;
    let profile_plane = Plane::from_axes(profile_plane.point_at(uv), profile_plane.x_axis, profile_plane.y_axis);
    let placement = initial_placement(profile_plane, base, start, tangent, options)?;
    let x = Vec3::from(placement.vector(profile_plane.x_axis));
    let y = Vec3::from(placement.vector(profile_plane.y_axis));
    let aligned_normal = x.cross(y).normalize()?;
    let first = Frame { origin: Vec3::from(placement.point(profile_plane.origin)), x, y };
    let up = aligned_normal.dot(tangent);
    if !sheet && up.abs() <= 1e-8 {
        return None;
    }
    let rail = sweep_rail_length(&source_wires,
        options.profile_axes.unwrap_or([profile_plane.x_axis, profile_plane.y_axis]), tangent)
        .map(|length| RailLaw { length, twist: options.twist });
    if lift.is_some() {
        let radius = wires.iter().flat_map(|wire| &wire.curves).flat_map(|curve| &curve.points)
            .map(|p| Vec3::from(profile_plane.point_at(*p)).distance(base))
            .fold(1.0_f64, f64::max) * options.scale.max(1.0);
        let (patches, runs) = transported_runs(&pieces, first, options, extent, radius, closed, rail)?;
        return build_body_in_runs(&wires, &patches, &runs, true, closed, true, false, lift);
    }
    if up.abs() > 1.0 - 1e-10 && rotationally_invariant(&source_wires) {
        // Turning a complete concentric circle changes its seam parameter,
        // not its geometry. Avoid a helical spline skin for the same cone or
        // tube: this is an exact symmetry reduction, including closed paths.
        options.twist = 0.0;
        options.bank = false;
    }
    if closed && ((options.scale - 1.0).abs() > 1e-10
        || (options.twist / TAU - (options.twist / TAU).round()).abs() > 1e-9)
    {
        return None;
    }

    // Retain cylinders, planes, cones and tori in the ordinary cases.
    if options.twist.abs() <= 1e-12 && (options.scale - 1.0).abs() <= 1e-12
        && pieces.len() == 1
    {
        let plane = first.plane();
        let analytic = match &pieces[0] {
            Piece::Line(a, b) if sheet && wires.len() == 1 =>
                super::sweep::extrude_surface(plane, &source_wires[0], (*b - *a).to_array()),
            Piece::Line(a, b) if !sheet =>
                super::sweep::extrude_region(plane, &source_wires, (*b - *a).to_array()),
            Piece::Planar(path_plane, Curve::Arc(arc), forward) => {
                let pivot = path_plane.point_at(arc.centre);
                let axis = path_plane.normal()?;
                let angle = arc.sweep() * if *forward { 1.0 } else { -1.0 };
                if sheet {
                    super::sweep::revolve_surface_region(plane, &source_wires, pivot, axis, angle)
                } else {
                    super::sweep::revolve_region(plane, &source_wires, pivot, axis, angle)
                }
            }
            _ => None,
        };
        if let Some(body) = analytic {
            // Quarter faces of a round profile share one surface: join them.
            let mut joined = body.clone();
            if analytic_ruled_faces(&mut joined).is_some() && joined.validate().is_empty() {
                return Some(joined);
            }
            return Some(body);
        }
    }

    // A round tube with a corner on a circular run keeps its runs exact and
    // joins them as the reference modeler does.
    if !sheet && wires.len() == 1 && options.twist.abs() <= 1e-12 && (options.scale - 1.0).abs() <= 1e-12 {
        if let Some((centre, radius)) = profile_circle(&source_wires[0]) {
            if let Some(body) = circular_tube(&pieces, first, centre, radius, closed) {
                return Some(body);
            }
        }
        if let Some(body) = planar_band(&pieces, first, &source_wires, closed) {
            return Some(body);
        }
        if let Some(body) = planar_polygon_band(&pieces, first, &source_wires, closed) {
            return Some(body);
        }
    }

    let radius = wires.iter().flat_map(|wire| &wire.curves)
        .flat_map(|curve| &curve.points)
        .map(|p| Vec3::from(profile_plane.point_at(*p)).distance(base))
        .fold(1.0_f64, f64::max) * options.scale.max(1.0);
    if closed && !sheet {
        if let Some(body) = closed_turned_polyline(&pieces, first, &source_wires, &wires, options, radius) {
            return Some(body);
        }
    }
    let (patches, runs) = transported_runs(&pieces, first, options,
        extent, radius, closed, rail)?;
    // A sheet need not sweep out any volume (for example an in-plane line
    // translated sideways), so only solid sections use the volume check.
    let deformed = options.twist.abs() > 1e-12 || (options.scale - 1.0).abs() > 1e-12;
    let outward = if sheet { up >= 0.0 } else { regular_transport(&wires, &patches, deformed)? };
    if options.twist.abs() <= 1e-12 && (options.scale - 1.0).abs() <= 1e-12 {
        let mut exact = build_body(&wires, &patches, sheet, closed, outward)?;
        let all_analytic = |body: &Body| body.faces.iter().all(|(_, face)|
            !body.surfaces.get(face.surface).is_some_and(Surface::is_spline));
        if analytic_ruled_faces(&mut exact).is_some() && exact.validate().is_empty() && all_analytic(&exact) {
            return Some(exact);
        }
    }
    // One face per profile curve and path run, as the reference builds it.
    let joined = build_body_in_runs(&wires, &patches, &runs, sheet, closed, outward, true, None)?;
    // A run face too large to triangulate falls back to one face per patch.
    // ponytail: probe meshing at display settings; replace with a cheaper size bound if this shows up in profiles.
    let meshes = super::mesh::tessellate(&joined, super::mesh::TessellationTolerance::new(0.05, 1e-5)).missing_faces.is_empty();
    if meshes { Some(joined) } else { build_body(&wires, &patches, sheet, closed, outward) }
}

/// Length of the rail the reference lays beside a scaled sweep's path
/// (`RailLaw`), measured on its records: half the profile's largest
/// coordinate from its anchor along its own plane axes, at most one; times,
/// unless the profile is a single circle, the length of the plane axis
/// least aligned with the path start once projected across the path.
/// `wires` are relative to the anchor.
fn sweep_rail_length(wires: &[Vec<Curve>], axes: [[f64; 3]; 2], tangent: Vec3) -> Option<f64> {
    let mut reach = 0.0_f64;
    for curve in wires.iter().flatten() {
        for sample in 0..=64 {
            let point = curve.point_at(sample as f64 / 64.0);
            reach = reach.max(point[0].abs()).max(point[1].abs());
        }
    }
    let lean = if wires.len() == 1 && profile_circle(&wires[0]).is_some() { 1.0 } else {
        let across = |axis: [f64; 3]| -> Option<f64> {
            let axis = Vec3::from(axis).normalize()?;
            Some((axis - tangent * axis.dot(tangent)).length())
        };
        across(axes[0])?.max(across(axes[1])?)
    };
    let length = (reach * 0.5).min(1.0) * lean;
    (length.is_finite() && length > 1e-12).then_some(length)
}

/// Faces swept along straight and circular runs without twist or scaling
/// lie on planes, cylinders, cones, spheres and tori. Store those surfaces
/// exactly and let neighbouring faces on one surface share a face, as the
/// reference modeler does: a round profile gives one tube per run, and a
/// flat side shared by coplanar runs one face.
fn analytic_ruled_faces(body: &mut Body) -> Option<()> {
    let size = body.vertices.iter()
        .flat_map(|(_, vertex)| vertex.point)
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let tolerance = size * 1e-9;
    // Straight spline edges become lines between their vertices.
    for key in body.edge_keys().collect::<Vec<_>>() {
        let edge = body.edges.get(key)?;
        let Some(Curve3::Nurbs(curve)) = body.curves.get(edge.curve) else { continue };
        let (start, end) = (Vec3::from(body.vertices.get(edge.start)?.point), Vec3::from(body.vertices.get(edge.end)?.point));
        let Some(direction) = (end - start).normalize() else { continue };
        let straight = curve.control_points().iter().all(|point| {
            let offset = Vec3::from(*point) - start;
            (offset - direction * offset.dot(direction)).length() <= tolerance
        });
        if !straight { continue; }
        let shared = body.edges.iter().filter(|(_, other)| other.curve == edge.curve).count() > 1;
        let line = Curve3::Line(super::geometry::Line3 { origin: start.to_array(), direction: (end - start).to_array() });
        let old = edge.curve;
        let new = body.curves.insert(line);
        let target = body.edges.get_mut(key)?;
        target.curve = new;
        target.start_parameter = 0.0;
        target.end_parameter = 1.0;
        if !shared { body.curves.remove(old); }
    }
    for face in body.face_keys().collect::<Vec<_>>() {
        let (old, forward) = body.faces.get(face).map(|face| (face.surface, face.forward))?;
        let Some(Surface::Nurbs(surface)) = body.surfaces.get(old) else { continue };
        let found = ruled_analytic(surface, tolerance).or_else(|| revolved_analytic(surface, tolerance));
        let Some((analytic, agrees)) = found else { continue };
        let surface = body.surfaces.insert(analytic);
        let target = body.faces.get_mut(face)?;
        target.surface = surface;
        target.forward = forward == agrees;
        body.surfaces.remove(old);
        for coedge in body.face_coedges(face) {
            body.coedges.get_mut(coedge)?.pcurve = None;
        }
    }
    loop {
        let joinable = body.edge_keys().find(|edge| joins_one_surface(body, *edge, tolerance));
        let Some(edge) = joinable else { break };
        dissolve_edge(body, edge)?;
    }
    Some(())
}

/// A plane or circular cylinder that a ruled patch lies on exactly, and
/// whether that surface's natural normal agrees with the patch's.
fn ruled_analytic(surface: &NurbsSurface3, tolerance: f64) -> Option<(Surface, bool)> {
    let mut direction: Option<Vec3> = None;
    for (row, weights) in surface.control_points().iter().zip(surface.weights()) {
        let start = Vec3::from(*row.first()?);
        let along = (Vec3::from(*row.last()?) - start).normalize()?;
        match direction {
            Some(direction) if direction.cross(along).length() > 1e-9 || direction.dot(along) < 0.0 => return None,
            Some(_) => {}
            None => direction = Some(along),
        }
        for (point, weight) in row.iter().zip(weights) {
            let offset = Vec3::from(*point) - start;
            if (offset - along * offset.dot(along)).length() > tolerance
                || (weight - weights[0]).abs() > weights[0].abs() * 1e-12
            {
                return None;
            }
        }
    }
    let direction = direction?;
    let origin = Vec3::from(surface.point_at(0.0, 0.0));
    let flat = |point: Vec3| point - direction * (point - origin).dot(direction);
    let samples = (0..=8).map(|step| flat(Vec3::from(surface.point_at(step as f64 / 8.0, 0.0))))
        .collect::<Vec<_>>();
    let natural = Vec3::from(surface.normal_at(0.5, 0.5)?);
    let middle = Vec3::from(surface.point_at(0.5, 0.5));
    let chord = samples.iter().map(|point| *point - origin).find(|chord| chord.length() > tolerance)?;
    let normal = direction.cross(chord).normalize()?;
    let mut controls = surface.control_points().iter().flatten().map(|point| Vec3::from(*point));
    if controls.all(|point| (point - origin).dot(normal).abs() <= tolerance) {
        let plane = Plane::orthonormal(origin.to_array(), direction.to_array(), normal.to_array())?;
        return Some((Surface::Plane(plane), normal.dot(natural) > 0.0));
    }
    // The circle through three distinct section points (a whole circle's
    // first and last coincide).
    let (a, b, c) = (samples[0], samples[3], samples[6]);
    let (ab, ac) = (b - a, c - a);
    let axis = ab.cross(ac);
    if axis.length() <= tolerance * tolerance.max(1.0) {
        return None;
    }
    let centre = a + (axis.cross(ab) * ac.dot(ac) + ac.cross(axis) * ab.dot(ab)) * (0.5 / axis.dot(axis));
    let radius = a.distance(centre);
    if axis.cross(direction).length() > axis.length() * 1e-9
        || samples.iter().any(|point| (point.distance(centre) - radius).abs() > tolerance)
    {
        return None;
    }
    let x_axis = (a - centre) * (1.0 / radius);
    let base = Plane::orthonormal(centre.to_array(), x_axis.to_array(), direction.to_array())?;
    let outward = flat(middle) - centre;
    Some((Surface::Cylinder(super::geometry::Cylinder { base, radius }), outward.dot(natural) > 0.0))
}

/// The plane, cylinder, cone, sphere or torus that a patch turned exactly
/// about one axis lies on, and whether its natural normal agrees with the
/// patch's.
fn revolved_analytic(surface: &NurbsSurface3, tolerance: f64) -> Option<(Surface, bool)> {
    // Each section point runs round a circle about the common axis.
    let mut axis: Option<(Vec3, Vec3)> = None;
    let mut meridian = Vec::new();
    for step in 0..=8 {
        let u = step as f64 / 8.0;
        let ring = [0.0, 0.25, 0.5, 0.75, 1.0].map(|v| Vec3::from(surface.point_at(u, v)));
        let (a, b, c) = (ring[0], ring[2], ring[4]);
        let (ab, ac) = (b - a, c - a);
        let normal = ab.cross(ac);
        if normal.length() <= tolerance * tolerance.max(1.0) {
            // A point on the axis stays put.
            if ring.iter().any(|point| point.distance(a) > tolerance) { return None; }
            meridian.push(a);
            continue;
        }
        let centre = a + (normal.cross(ab) * ac.dot(ac) + ac.cross(normal) * ab.dot(ab)) * (0.5 / normal.dot(normal));
        let radius = a.distance(centre);
        if ring.iter().any(|point| (point.distance(centre) - radius).abs() > tolerance) { return None; }
        let direction = normal.normalize()?;
        match axis {
            Some((origin, along)) => {
                let offset = centre - origin;
                if along.cross(direction).length() > 1e-9
                    || (offset - along * offset.dot(along)).length() > tolerance
                {
                    return None;
                }
            }
            None => axis = Some((centre, direction)),
        }
        meridian.push(a);
    }
    let (origin, along) = axis?;
    // Section points as (distance from the axis, height along it).
    let polar = |point: Vec3| {
        let offset = point - origin;
        let height = offset.dot(along);
        ((offset - along * height).length(), height)
    };
    let section = meridian.iter().map(|point| polar(*point)).collect::<Vec<_>>();
    let radial = (meridian[0] - origin) - along * (meridian[0] - origin).dot(along);
    let x_axis = radial.normalize().or_else(|| along.cross(Vec3::X).normalize()).or_else(|| along.cross(Vec3::Y).normalize())?;
    let natural = Vec3::from(surface.normal_at(0.5, 0.5)?);
    let middle = Vec3::from(surface.point_at(0.5, 0.5));
    let (middle_radius, middle_height) = polar(middle);
    let middle_radial = ((middle - origin) - along * middle_height).normalize()?;
    let (r0, h0) = section[0];
    let (r1, h1) = section[8];
    let frame_at = |height: f64| Plane::orthonormal((origin + along * height).to_array(), x_axis.to_array(), along.to_array());
    if section.iter().all(|(radius, _)| (radius - r0).abs() <= tolerance) && r0 > tolerance {
        let base = frame_at(h0)?;
        let surface = Surface::Cylinder(super::geometry::Cylinder { base, radius: r0 });
        return Some((surface, middle_radial.dot(natural) > 0.0));
    }
    if section.iter().all(|(_, height)| (height - h0).abs() <= tolerance) {
        let plane = frame_at(h0)?;
        return Some((Surface::Plane(plane), along.dot(natural) > 0.0));
    }
    // A straight meridian: a cone.
    let chord = (r1 - r0, h1 - h0);
    let length = chord.0.hypot(chord.1);
    let off_line = |(radius, height): (f64, f64)| ((radius - r0) * chord.1 - (height - h0) * chord.0).abs() / length;
    if length > tolerance && section.iter().all(|point| off_line(*point) <= tolerance) {
        let slope = chord.0 / chord.1;
        let half_angle = (-slope).atan();
        let (radius, height) = if r0 >= r1 { (r0, h0) } else { (r1, h1) };
        let base = frame_at(height)?;
        let outward = middle_radial + along * half_angle.tan();
        let surface = Surface::Cone(super::geometry::Cone { base, radius, half_angle });
        return Some((surface, outward.dot(natural) > 0.0));
    }
    // A circular meridian: a torus, or a sphere about a centre on the axis.
    let (a, b, c) = (section[0], section[3], section[6]);
    let determinant = 2.0 * (a.0 * (b.1 - c.1) + b.0 * (c.1 - a.1) + c.0 * (a.1 - b.1));
    if determinant.abs() <= tolerance * tolerance.max(1.0) { return None; }
    let square = |(x, y): (f64, f64)| x * x + y * y;
    let centre = (
        (square(a) * (b.1 - c.1) + square(b) * (c.1 - a.1) + square(c) * (a.1 - b.1)) / determinant,
        (square(a) * (c.0 - b.0) + square(b) * (a.0 - c.0) + square(c) * (b.0 - a.0)) / determinant,
    );
    let minor = (a.0 - centre.0).hypot(a.1 - centre.1);
    if section.iter().any(|point| ((point.0 - centre.0).hypot(point.1 - centre.1) - minor).abs() > tolerance) {
        return None;
    }
    let outward = (middle_radial * (middle_radius - centre.0) + along * (middle_height - centre.1)).normalize()?;
    let agrees = outward.dot(natural) > 0.0;
    let frame = frame_at(centre.1)?;
    if centre.0.abs() <= tolerance {
        return Some((Surface::Sphere(super::geometry::Sphere { frame, radius: minor }), agrees));
    }
    let torus = super::geometry::Torus { frame, major_radius: centre.0, minor_radius: minor };
    Some((Surface::Torus(torus), agrees))
}

/// Whether an edge separates two faces of one analytic surface, both facing
/// the same way, so that it can be removed.
fn joins_one_surface(body: &Body, edge: EdgeKey, tolerance: f64) -> bool {
    let Some(edge) = body.edges.get(edge) else { return false };
    let [first, second] = edge.coedges[..] else { return false };
    let side = |coedge| {
        let ring = body.coedges.get(coedge)?.owner;
        let face = body.loops.get(ring)?.owner;
        let value = body.faces.get(face)?;
        Some((face, ring, value.forward, body.surfaces.get(value.surface)?))
    };
    let (Some((face_a, loop_a, forward_a, surface_a)), Some((face_b, loop_b, forward_b, surface_b))) =
        (side(first), side(second))
    else {
        return false;
    };
    if face_a == face_b && loop_a != loop_b {
        return false;
    }
    match (surface_a, surface_b) {
        (Surface::Plane(a), Surface::Plane(b)) => {
            let (Some(normal_a), Some(normal_b)) = (a.normal(), b.normal()) else { return false };
            let (normal_a, normal_b) = (Vec3::from(normal_a), Vec3::from(normal_b));
            let facing = if forward_a == forward_b { 1.0 } else { -1.0 };
            normal_a.dot(normal_b) * facing > 1.0 - 1e-12
                && (Vec3::from(b.origin) - Vec3::from(a.origin)).dot(normal_a).abs() <= tolerance
        }
        (Surface::Cylinder(a), Surface::Cylinder(b)) => {
            let (Some(axis_a), Some(axis_b)) = (a.base.normal(), b.base.normal()) else { return false };
            let (axis_a, axis_b) = (Vec3::from(axis_a), Vec3::from(axis_b));
            let offset = Vec3::from(b.base.origin) - Vec3::from(a.base.origin);
            forward_a == forward_b
                && (a.radius - b.radius).abs() <= tolerance
                && axis_a.cross(axis_b).length() <= 1e-9
                && (offset - axis_a * offset.dot(axis_a)).length() <= tolerance
        }
        (Surface::Cone(a), Surface::Cone(b)) => {
            let (Some(axis_a), Some(axis_b)) = (a.base.normal(), b.base.normal()) else { return false };
            let (axis_a, axis_b) = (Vec3::from(axis_a), Vec3::from(axis_b));
            let offset = Vec3::from(b.base.origin) - Vec3::from(a.base.origin);
            let radius_a_at_b = a.radius - offset.dot(axis_a) * a.half_angle.tan();
            forward_a == forward_b
                && axis_a.dot(axis_b) > 1.0 - 1e-12
                && (a.half_angle - b.half_angle).abs() <= 1e-9
                && (offset - axis_a * offset.dot(axis_a)).length() <= tolerance
                && (radius_a_at_b - b.radius).abs() <= tolerance
        }
        (Surface::Sphere(a), Surface::Sphere(b)) => {
            forward_a == forward_b
                && (a.radius - b.radius).abs() <= tolerance
                && Vec3::from(a.frame.origin).distance(Vec3::from(b.frame.origin)) <= tolerance
        }
        (Surface::Torus(a), Surface::Torus(b)) => {
            let (Some(axis_a), Some(axis_b)) = (a.frame.normal(), b.frame.normal()) else { return false };
            forward_a == forward_b
                && Vec3::from(axis_a).cross(Vec3::from(axis_b)).length() <= 1e-9
                && Vec3::from(a.frame.origin).distance(Vec3::from(b.frame.origin)) <= tolerance
                && (a.major_radius - b.major_radius).abs() <= tolerance
                && (a.minor_radius - b.minor_radius).abs() <= tolerance
        }
        _ => false,
    }
}

/// Removes an edge between two faces of one surface: their loops are joined,
/// or, when both uses already belong to one loop, that loop is split in two.
fn dissolve_edge(body: &mut Body, edge: EdgeKey) -> Option<()> {
    let [first, second] = body.edges.get(edge)?.coedges[..] else { return None };
    let loop_a = body.coedges.get(first)?.owner;
    let loop_b = body.coedges.get(second)?.owner;
    let face_a = body.loops.get(loop_a)?.owner;
    let face_b = body.loops.get(loop_b)?.owner;
    // A ring rotated to start just after `coedge`, without it.
    let after = |ring: &[CoedgeKey], coedge: CoedgeKey| -> Option<Vec<CoedgeKey>> {
        let at = ring.iter().position(|key| *key == coedge)?;
        Some(ring[at + 1..].iter().chain(&ring[..at]).copied().collect())
    };
    if loop_a == loop_b {
        let ring = after(&body.loops.get(loop_a)?.coedges, first)?;
        let split = ring.iter().position(|key| *key == second)?;
        let (inner, outer) = (ring[..split].to_vec(), ring[split + 1..].to_vec());
        // Two uses running straight back on each other leave one ring, or
        // none at all when they were the whole of it.
        let (keep, moved) = if loop_area(body, face_a, &inner)? > loop_area(body, face_a, &outer)? {
            (inner, outer)
        } else {
            (outer, inner)
        };
        if keep.is_empty() {
            body.loops.remove(loop_a);
            body.faces.get_mut(face_a)?.loops.retain(|key| *key != loop_a);
        } else {
            body.loops.get_mut(loop_a)?.coedges = keep;
        }
        if !moved.is_empty() {
            let ring = body.loops.insert(Loop { coedges: moved.clone(), owner: face_a, provenance: Provenance::Synthesized });
            for coedge in moved {
                body.coedges.get_mut(coedge)?.owner = ring;
            }
            body.faces.get_mut(face_a)?.loops.push(ring);
        }
    } else {
        let mut ring = after(&body.loops.get(loop_a)?.coedges, first)?;
        let tail = after(&body.loops.get(loop_b)?.coedges, second)?;
        for coedge in &tail {
            body.coedges.get_mut(*coedge)?.owner = loop_a;
        }
        ring.extend(tail);
        body.loops.get_mut(loop_a)?.coedges = ring;
        body.loops.remove(loop_b);
        let others = body.faces.get(face_b)?.loops.iter().copied().filter(|key| *key != loop_b).collect::<Vec<_>>();
        for other in others {
            body.loops.get_mut(other)?.owner = face_a;
            body.faces.get_mut(face_a)?.loops.push(other);
        }
        let face = body.faces.remove(face_b)?;
        body.shells.get_mut(face.owner)?.faces.retain(|key| *key != face_b);
        body.surfaces.remove(face.surface);
    }
    body.coedges.remove(first);
    body.coedges.remove(second);
    let removed = body.edges.remove(edge)?;
    if !body.edges.iter().any(|(_, edge)| edge.curve == removed.curve) {
        body.curves.remove(removed.curve);
    }
    for vertex in [removed.start, removed.end] {
        if !body.edges.iter().any(|(_, edge)| edge.start == vertex || edge.end == vertex) {
            body.vertices.remove(vertex);
        }
    }
    Some(())
}

/// The area a loop encloses across its face's axis, from the polygon of its
/// coedge start points; it only orders the two loops a split leaves.
fn loop_area(body: &Body, face: FaceKey, ring: &[CoedgeKey]) -> Option<f64> {
    let normal = match body.surfaces.get(body.faces.get(face)?.surface)? {
        Surface::Plane(plane) => Vec3::from(plane.normal()?),
        Surface::Cylinder(cylinder) => Vec3::from(cylinder.base.normal()?),
        _ => return Some(0.0),
    };
    let points = ring.iter()
        .map(|coedge| {
            let (start, _) = body.coedge_vertices(*coedge)?;
            Some(Vec3::from(body.vertices.get(start)?.point))
        })
        .collect::<Option<Vec<_>>>()?;
    let area = (0..points.len()).fold(Vec3::ZERO, |sum, index| {
        sum + points[index].cross(points[(index + 1) % points.len()])
    });
    Some(area.dot(normal).abs() * 0.5)
}

/// The centre and radius of a profile that is one whole circle.
fn profile_circle(wire: &[Curve]) -> Option<([f64; 2], f64)> {
    let pieces = expanded(wire)?;
    let Curve::Arc(first) = pieces.first()? else { return None };
    let mut sweep = 0.0;
    for piece in &pieces {
        let Curve::Arc(arc) = piece else { return None };
        let scale = first.radius.abs().max(1.0) * 1e-10;
        if (arc.centre[0] - first.centre[0]).hypot(arc.centre[1] - first.centre[1]) > scale
            || (arc.radius - first.radius).abs() > scale
        {
            return None;
        }
        sweep += arc.sweep();
    }
    ((sweep - TAU).abs() <= 1e-9 && first.radius > 0.0).then_some((first.centre, first.radius))
}

/// One run of a tube: straight, or turning about an axis.
#[derive(Clone, Copy)]
enum TubeRun {
    Straight { from: Vec3, to: Vec3 },
    Turn { centre: Vec3, axis: Vec3, angle: f64 },
}

/// The circular section of a tube at a frame.
struct TubeSection { centre: Vec3, normal: Vec3, x: Vec3 }

fn tube_section(frame: Frame, centre: [f64; 2], tangent: Vec3) -> Option<TubeSection> {
    Some(TubeSection { centre: frame.point(centre), normal: tangent, x: frame.x.normalize()? })
}

/// A run's swept solid, for point membership near a corner: the infinite
/// cylinder or the whole torus, limited to the run's own extent.
struct TubeSolid { run: TubeRun, start: Vec3, radius: f64 }

impl TubeSolid {
    /// Signed distance to the tube surface (negative inside) and whether the
    /// point lies within the run's extent.
    fn probe(&self, point: Vec3) -> (f64, bool) {
        match self.run {
            TubeRun::Straight { from, to } => {
                let direction = (to - from).normalize().unwrap_or(Vec3::X);
                let length = from.distance(to);
                let offset = point - self.start;
                let along = offset.dot(direction);
                let across = (offset - direction * along).length();
                (across - self.radius, (-1e-12..=length + 1e-12).contains(&along))
            }
            TubeRun::Turn { centre, axis, angle } => {
                let radial = self.start - centre;
                let height = radial.dot(axis);
                let base = centre + axis * height;
                let first = (self.start - base).normalize().unwrap_or(Vec3::X);
                let offset = point - base;
                let z = offset.dot(axis);
                let flat = offset - axis * z;
                let major = (self.start - base).length();
                let gap = ((flat.length() - major).powi(2) + z * z).sqrt() - self.radius;
                let mut turned = axis.dot(first.cross(flat)).atan2(first.dot(flat));
                if turned < -1e-12 { turned += TAU; }
                (gap, turned <= angle + 1e-12)
            }
        }
    }

    fn contains(&self, point: Vec3) -> bool {
        let (gap, within) = self.probe(point);
        gap < 0.0 && within
    }
}

/// A point of a run's surface swept from a section point, `t` in [0, 1]
/// along the run.
fn run_point(run: TubeRun, section: Vec3, t: f64) -> Vec3 {
    match run {
        TubeRun::Straight { from, to } => section + (to - from) * t,
        TubeRun::Turn { centre, axis, angle } => centre + rotate(section - centre, axis, angle * t),
    }
}

fn run_length(run: TubeRun, section: Vec3) -> f64 {
    match run {
        TubeRun::Straight { from, to } => from.distance(to),
        TubeRun::Turn { centre, axis, angle } => {
            let offset = section - centre;
            (offset - axis * offset.dot(axis)).length() * angle
        }
    }
}

fn tube_surface(run: TubeRun, section: &TubeSection, radius: f64) -> Option<Surface> {
    match run {
        TubeRun::Straight { .. } => Some(Surface::Cylinder(super::geometry::Cylinder {
            base: Plane::orthonormal(section.centre.to_array(), section.x.to_array(), section.normal.to_array())?,
            radius,
        })),
        TubeRun::Turn { centre, axis, angle } => {
            let offset = section.centre - centre;
            let base = centre + axis * offset.dot(axis);
            let radial = section.centre - base;
            let major = radial.length();
            if major <= radius * (1.0 + 1e-9) { return None; }
            Some(Surface::Torus(super::geometry::Torus {
                // The seam lies opposite the middle of the run, away from its ends.
                frame: Plane::orthonormal(base.to_array(), (rotate(radial, axis, angle * 0.5) * (1.0 / major)).to_array(), axis.to_array())?,
                major_radius: major,
                minor_radius: radius,
            }))
        }
    }
}

fn circle_curve(section: &TubeSection, radius: f64) -> Option<Curve3> {
    Some(Curve3::Circle(Circle3 {
        plane: Plane::orthonormal(section.centre.to_array(), section.x.to_array(), section.normal.to_array())?,
        radius,
    }))
}

/// Builds a circular tube along straight and circular runs meeting at
/// corners. Each run keeps its exact cylinder or torus. At a corner the
/// outer side is closed by straight extensions of both runs to the mitre
/// plane; on the inner side the two runs meet along the curve where their
/// surfaces cross (the rest of the mitre between two straight runs), which
/// is how the reference modeler joins a round tube at a corner.
fn circular_tube(pieces: &[Piece], first: Frame, centre: [f64; 2], radius: f64, closed: bool) -> Option<Body> {
    let runs = pieces.iter().map(|piece| match piece {
        Piece::Line(from, to) => Some(TubeRun::Straight { from: *from, to: *to }),
        Piece::Planar(..) => circular_run(piece).map(|(centre, axis, angle)| TubeRun::Turn { centre, axis, angle }),
        Piece::Spline(..) => None,
    }).collect::<Option<Vec<_>>>()?;
    let count = runs.len();
    let joints = if closed { count } else { count - 1 };
    let corner = |index: usize| -> Option<bool> {
        let (before, after) = (&pieces[index], &pieces[(index + 1) % count]);
        Some(is_corner(before.tangent(1.0)?, after.tangent(0.0)?))
    };
    let mut wanted = false;
    for joint in 0..joints {
        wanted |= corner(joint)?;
    }
    if !wanted { return None; }

    // Section frames at both ends of every run.
    let mut starts = Vec::with_capacity(count);
    let mut ends = Vec::with_capacity(count);
    let mut frame = first;
    for (index, (piece, run)) in pieces.iter().zip(&runs).enumerate() {
        if index > 0 {
            let point = piece.point(0.0);
            frame = moved(frame, pieces[index - 1].tangent(1.0)?, piece.tangent(0.0)?, point, point)?;
        }
        if (frame.x.cross(frame.y).normalize()?.dot(piece.tangent(0.0)?) - 1.0).abs() > 1e-9
            || (frame.x.length() - 1.0).abs() > 1e-9 || (frame.y.length() - 1.0).abs() > 1e-9
            || frame.x.dot(frame.y).abs() > 1e-9
        {
            return None;
        }
        starts.push(frame);
        frame = match *run {
            TubeRun::Straight { from, to } => Frame { origin: frame.origin + (to - from), ..frame },
            TubeRun::Turn { centre, axis, angle } => Frame {
                origin: centre + rotate(frame.origin - centre, axis, angle),
                x: rotate(frame.x, axis, angle),
                y: rotate(frame.y, axis, angle),
            },
        };
        ends.push(frame);
    }
    let size = starts.iter().chain(&ends).map(|frame| frame.origin.length()).fold(radius, f64::max);
    let tolerance = size * 1e-10;
    if closed {
        let point = pieces[0].point(0.0);
        let back = moved(ends[count - 1], pieces[count - 1].tangent(1.0)?, pieces[0].tangent(0.0)?, point, point)?;
        if frame_error(back, starts[0], radius) > tolerance * 10.0 { return None; }
    }

    let mut body = Body::new();
    let lump = body.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
    let shell = body.shells.insert(Shell { faces: Vec::new(), owner: lump, provenance: Provenance::Synthesized });
    let vertex = |body: &mut Body, point: Vec3| body.vertices.insert(Vertex { point: point.to_array(), provenance: Provenance::Synthesized });
    let edge = |body: &mut Body, curve: Curve3, from: VertexKey, to: VertexKey, low: f64, high: f64| {
        let curve = body.curves.insert(curve);
        body.edges.insert(Edge { curve, start_parameter: low, end_parameter: high, start: from, end: to, coedges: Vec::new(), provenance: Provenance::Synthesized })
    };
    // A whole circle as one closed edge.
    let full_circle = |body: &mut Body, section: &TubeSection| -> Option<EdgeKey> {
        let point = section.centre + section.x * radius;
        let at = vertex(body, point);
        Some(edge(body, circle_curve(section, radius)?, at, at, 0.0, TAU))
    };
    // The arc of a closed curve between two points that contains `through`.
    let arc_edge = |body: &mut Body, curve: Curve3, from: (VertexKey, Vec3), to: (VertexKey, Vec3), through: Vec3| -> Option<EdgeKey> {
        let wrap = |value: f64, base: f64| base + (value - base).rem_euclid(TAU);
        let a = curve.parameter_at(from.1.to_array());
        let b = wrap(curve.parameter_at(to.1.to_array()), a);
        let m = wrap(curve.parameter_at(through.to_array()), a);
        Some(if m < b {
            edge(body, curve, from.0, to.0, a, b)
        } else {
            let start = curve.parameter_at(to.1.to_array());
            let end = wrap(a, start);
            edge(body, curve, to.0, from.0, start, end)
        })
    };

    // Per run: the loop at each end, as edges in order, and the faces.
    let mut start_loops: Vec<Vec<EdgeKey>> = vec![Vec::new(); count];
    let mut end_loops: Vec<Vec<EdgeKey>> = vec![Vec::new(); count];
    // Extension faces: surface, loop edges, and the side vector at an edge.
    let mut extensions: Vec<(Surface, Vec<EdgeKey>, Vec3)> = Vec::new();
    // The inner curves' images on the two runs' surfaces, by edge and run.
    let mut traces: Vec<(EdgeKey, usize, NurbsCurve)> = Vec::new();
    for joint in 0..joints {
        let (a, b) = (joint, (joint + 1) % count);
        let point = pieces[b].point(0.0);
        let d1 = pieces[a].tangent(1.0)?;
        let d2 = pieces[b].tangent(0.0)?;
        let end = tube_section(ends[a], centre, d1)?;
        let start = tube_section(starts[b], centre, d2)?;
        if !corner(joint)? {
            let rim = full_circle(&mut body, &end)?;
            end_loops[a].push(rim);
            start_loops[b].push(rim);
            continue;
        }
        let normal = (d1 + d2).normalize()?;
        let binormal = d1.cross(d2).normalize()?;
        // The section points on the corner's binormal line split the outer
        // side, closed by the extensions, from the inner side.
        let offset = end.centre - point;
        let along = offset.dot(binormal);
        let away = (offset - binormal * along).length();
        if away >= radius * (1.0 - 1e-9) { return None; }
        let half = (radius * radius - away * away).sqrt();
        let j1 = point + binormal * (along - half);
        let j2 = point + binormal * (along + half);
        let (v1, v2) = (vertex(&mut body, j1), vertex(&mut body, j2));
        // Outer and inner middle points of the end section.
        let outer_dir = (-d2 + d1 * d2.dot(d1)).normalize()?;
        let outer_end = end.centre + (outer_dir * radius);
        let outer_start = start.centre + ((d1 - d2 * d1.dot(d2)).normalize()? * radius);
        // The mitre ellipse: the end section carried along d1 onto the plane.
        let tilt = (normal - d1 * normal.dot(d1)).normalize()?;
        let side = d1.cross(tilt);
        let lift = |vector: Vec3| vector - d1 * (vector.dot(normal) / d1.dot(normal));
        let shift = -(end.centre - point).dot(normal) / d1.dot(normal);
        let ellipse_centre = end.centre + d1 * shift;
        let major = lift(tilt * radius);
        let ellipse_plane = Plane::orthonormal(ellipse_centre.to_array(), major.normalize()?.to_array(), major.cross(side).normalize()?.to_array())?;
        let ellipse_again = || Curve3::Ellipse(super::geometry::Ellipse3 { plane: ellipse_plane, major_radius: major.length(), minor_radius: radius });
        let ellipse = ellipse_again();
        let outer_mitre = outer_end + d1 * (-(outer_end - point).dot(normal) / d1.dot(normal));
        let mitre = arc_edge(&mut body, ellipse, (v1, j1), (v2, j2), outer_mitre)?;
        // The inner curve: the rest of the mitre between two straight runs,
        // else where each section point of run b, carried along its run,
        // leaves run a.
        let both_straight = matches!(runs[a], TubeRun::Straight { .. }) && matches!(runs[b], TubeRun::Straight { .. });
        let inner_end = end.centre - outer_dir * radius;
        let inner_mitre = inner_end + d1 * (-(inner_end - point).dot(normal) / d1.dot(normal));
        let inner = if both_straight {
            vec![arc_edge(&mut body, ellipse_again(), (v2, j2), (v1, j1), inner_mitre)?]
        } else {
        let solid = TubeSolid { run: runs[a], start: tube_section(starts[a], centre, pieces[a].tangent(0.0)?)?.centre, radius };
        let inner_dir = -((d1 - d2 * d1.dot(d2)).normalize()?);
        let from = j2 - start.centre;
        let to = j1 - start.centre;
        // Turn from j2 to j1 through the inner middle point.
        let mut span = d2.dot(from.cross(to)).atan2(from.dot(to));
        let middle = d2.dot(from.cross(inner_dir)).atan2(from.dot(inner_dir));
        let between = if span > 0.0 { middle > 0.0 && middle < span } else { middle < 0.0 && middle > span };
        if !between {
            span += if span > 0.0 { -TAU } else { TAU };
        }
        let surface_a = tube_surface(runs[a], &tube_section(starts[a], centre, pieces[a].tangent(0.0)?)?, radius)?;
        let surface_b = tube_surface(runs[b], &start, radius)?;
        // Where the section point at `share` of the turn from j2 to j1,
        // carried along run b, leaves tube a.
        let cross = |share: f64| -> Option<Vec3> {
            let section_point = start.centre + rotate(from, d2, span * share);
            let length = run_length(runs[b], section_point).max(radius);
            let steps = ((length / (radius / 64.0)).ceil() as usize).clamp(16, 100_000);
            let mut inside_at = 0.0;
            let mut outside_at = None;
            for k in 1..=steps {
                let t = k as f64 / steps as f64;
                if !solid.contains(run_point(runs[b], section_point, t)) { outside_at = Some(t); break; }
                inside_at = t;
            }
            let (mut low, mut high) = (inside_at, outside_at?);
            for _ in 0..80 {
                let mid = 0.5 * (low + high);
                if solid.contains(run_point(runs[b], section_point, mid)) { low = mid } else { high = mid }
            }
            let found = run_point(runs[b], section_point, 0.5 * (low + high));
            let (gap, within) = solid.probe(found);
            (gap.abs() <= tolerance * 1e3 && within).then_some(found)
        };
        // The crossing is interpolated through points on both tubes, evenly
        // spaced in the turn and with the tangents of the true curve at the
        // ends of each half (a natural end loses two orders of accuracy).
        // ACIS takes it as an exact curve, so it must stay on both surfaces
        // to well within the modeller's resolution (1e-6) between the points
        // too: the sampling is refined until it does.
        let fit_tolerance = 1e-8 * radius.max(1.0);
        let off_surfaces = |point: Vec3| -> Option<f64> {
            let (u, v) = surface_b.parameters_at(point.to_array())?;
            Some(solid.probe(point).0.abs().max(Vec3::from(surface_b.point_at(u, v)).distance(point)))
        };
        // d point / d share, one-sided into the half it ends.
        let slope = |share: f64, at: Vec3, inward: f64| -> Option<Vec3> {
            let h = 1e-4 * inward;
            let (near, far) = (cross(share + h)?, cross(share + 2.0 * h)?);
            Some((at * -3.0 + near * 4.0 - far) / (2.0 * h))
        };
        let mut fitted = None;
        for samples in [48usize, 96, 192, 384, 768, 1536] {
            let mut points = vec![j2];
            for step in 1..samples {
                points.push(cross(step as f64 / samples as f64)?);
            }
            points.push(j1);
            // Two halves, split where the curve reaches deepest into the corner.
            let middle = samples / 2;
            let middle_share = middle as f64 / samples as f64;
            let ends = [(0.0, &points[0], 1.0), (middle_share, &points[middle], -1.0), (middle_share, &points[middle], 1.0), (1.0, &points[samples], -1.0)];
            // Tangents per unit of the uniform parameter (one per sample).
            let tangents = ends.iter().map(|(share, at, inward)| Some(slope(*share, **at, *inward)? / samples as f64)).collect::<Option<Vec<_>>>()?;
            let halves = [(0, middle, tangents[0], tangents[1]), (middle, samples, tangents[2], tangents[3])];
            let mut worst: f64 = 0.0;
            let mut curves = Vec::new();
            for (from_index, to_index, start_tangent, end_tangent) in halves {
                let part = &points[from_index..=to_index];
                let crossing = NurbsCurve3::interpolate_fit(&part.iter().map(|p| p.to_array()).collect::<Vec<_>>(), Some(start_tangent.to_array()), Some(end_tangent.to_array()), ocs_geom::space::Parameterization::Uniform)?;
                // point_at takes the parameter normalised to [0, 1].
                for step in 0..part.len() - 1 {
                    let t = (step as f64 + 0.5) / (part.len() - 1) as f64;
                    worst = worst.max(off_surfaces(Vec3::from(crossing.point_at(t)))?);
                }
                curves.push((part.to_vec(), crossing, start_tangent, end_tangent));
            }
            if worst <= fit_tolerance || samples == 1536 {
                fitted = Some(curves);
                break;
            }
        }
        let curves = fitted?;
        let vm = vertex(&mut body, curves[0].0[curves[0].0.len() - 1]);
        let mut halves = Vec::new();
        for ((part, crossing, start_tangent, end_tangent), (from, to)) in curves.into_iter().zip([(v2, vm), (vm, v1)]) {
            let (low, high) = crossing.domain();
            let key = edge(&mut body, Curve3::Nurbs(crossing), from, to, low, high);
            // The same points in each surface's parameters, interpolated with
            // the same parameter values and end tangents, so each face
            // carries the same trace.
            for (run, surface) in [(a, &surface_a), (b, &surface_b)] {
                let mut uv: Vec<[f64; 2]> = Vec::with_capacity(part.len());
                for point in part.iter() {
                    let (mut u, mut v) = surface.parameters_at(point.to_array())?;
                    if let Some(&[pu, pv]) = uv.last() {
                        u += TAU * ((pu - u) / TAU).round();
                        if matches!(surface, Surface::Torus(_)) { v += TAU * ((pv - v) / TAU).round(); }
                    }
                    uv.push([u, v]);
                }
                // The parameter-space tangent: the spatial tangent through the
                // surface's inverse at each end.
                let uv_tangent = |point: Vec3, tangent: Vec3, at: [f64; 2]| -> Option<[f64; 2]> {
                    let h = 1e-6 / tangent.length().max(1e-12);
                    let (mut u, mut v) = surface.parameters_at((point + tangent * h).to_array())?;
                    u += TAU * ((at[0] - u) / TAU).round();
                    if matches!(surface, Surface::Torus(_)) { v += TAU * ((at[1] - v) / TAU).round(); }
                    Some([(u - at[0]) / h, (v - at[1]) / h])
                };
                let first = uv[0];
                let last = uv[uv.len() - 1];
                let start_uv = uv_tangent(part[0], start_tangent, first)?;
                let end_uv = uv_tangent(part[part.len() - 1], end_tangent, last)?;
                let trace = NurbsCurve::interpolate(&uv, Some(start_uv), Some(end_uv), ocs_geom::space::Parameterization::Uniform)?;
                let (start_knot, end_knot) = trace.domain();
                let knots = trace.knots().iter().map(|k| low + (k - start_knot) / (end_knot - start_knot) * (high - low)).collect();
                let trace = NurbsCurve::new_strict(trace.degree(), trace.control_points().to_vec(), knots, trace.weights().to_vec())?;
                traces.push((key, run, trace));
            }
            halves.push(key);
        }
        halves
        };
        // Run a's end: its own outer section edge when it turns, else the
        // mitre (its straight extension is the same cylinder).
        if matches!(runs[a], TubeRun::Turn { .. }) {
            let rim = arc_edge(&mut body, circle_curve(&end, radius)?, (v1, j1), (v2, j2), outer_end)?;
            end_loops[a].push(rim);
            end_loops[a].extend(inner.iter().copied());
            extensions.push((tube_surface(TubeRun::Straight { from: point, to: point + d1 }, &end, radius)?, vec![rim, mitre], d1));
        } else {
            end_loops[a].push(mitre);
            end_loops[a].extend(inner.iter().copied());
        }
        if matches!(runs[b], TubeRun::Turn { .. }) {
            let rim = arc_edge(&mut body, circle_curve(&start, radius)?, (v1, j1), (v2, j2), outer_start)?;
            start_loops[b].push(rim);
            start_loops[b].extend(inner.iter().copied());
            extensions.push((tube_surface(TubeRun::Straight { from: point, to: point + d2 }, &start, radius)?, vec![mitre, rim], d2));
        } else {
            start_loops[b].push(mitre);
            start_loops[b].extend(inner.iter().copied());
        }
    }
    let mut caps = Vec::new();
    if !closed {
        let start = tube_section(starts[0], centre, pieces[0].tangent(0.0)?)?;
        let finish = tube_section(ends[count - 1], centre, pieces[count - 1].tangent(1.0)?)?;
        let first_rim = full_circle(&mut body, &start)?;
        let last_rim = full_circle(&mut body, &finish)?;
        start_loops[0].push(first_rim);
        end_loops[count - 1].push(last_rim);
        caps.push((start, first_rim, -1.0));
        caps.push((finish, last_rim, 1.0));
    }

    // Faces. A loop is oriented so that the face lies on its left seen from
    // outside: `side` points from the loop's first edge into the face.
    let orient = |body: &Body, edges: &[EdgeKey], normal_at: &dyn Fn(Vec3) -> Vec3, side: Vec3| -> Option<Vec<(EdgeKey, bool)>> {
        let mut circuit = Vec::new();
        let first = body.edges.get(edges[0])?;
        let mut at = first.end;
        circuit.push((edges[0], true));
        // Chain the rest by shared vertices, whatever order they came in.
        let mut rest = edges[1..].to_vec();
        while !rest.is_empty() {
            let index = rest.iter().position(|key| body.edges.get(*key).is_some_and(|node| node.start == at || node.end == at))?;
            let key = rest.remove(index);
            let node = body.edges.get(key)?;
            if node.start == at { circuit.push((key, true)); at = node.end; } else { circuit.push((key, false)); at = node.start; }
        }
        let curve = body.curves.get(first.curve)?;
        let mid = 0.5 * (first.start_parameter + first.end_parameter);
        let point = Vec3::from(curve.point_at(mid));
        let walk = Vec3::from(curve.tangent_at(mid));
        if normal_at(point).cross(walk).dot(side) < 0.0 {
            circuit = circuit.into_iter().rev().map(|(edge, forward)| (edge, !forward)).collect();
        }
        Some(circuit)
    };
    let add_surface_face = |body: &mut Body, surface: Surface, loops: Vec<(Vec<EdgeKey>, Vec3)>, run: Option<usize>| -> Option<()> {
        let normal_surface = surface.clone();
        let key = body.surfaces.insert(surface);
        let face = add_face(body, shell, key, true);
        for (edges, side) in loops {
            let normal_at = |point: Vec3| -> Vec3 { outward_normal(&normal_surface, point) };
            let circuit = orient(body, &edges, &normal_at, side)?;
            let ring = add_loop(body, face, &circuit, None)?;
            for coedge in body.loops.get(ring)?.coedges.clone() {
                let node = body.coedges.get(coedge)?;
                let trace = traces.iter().find(|(key, owner, _)| *key == node.edge && Some(*owner) == run);
                if let Some((_, _, curve)) = trace {
                    let curve = Curve::Nurbs(if node.forward { curve.clone() } else { curve.reversed() });
                    body.coedges.get_mut(coedge)?.pcurve = Some(curve);
                }
            }
        }
        Some(())
    };
    for index in 0..count {
        let section = tube_section(starts[index], centre, pieces[index].tangent(0.0)?)?;
        let surface = tube_surface(runs[index], &section, radius)?;
        let loops = vec![
            (start_loops[index].clone(), pieces[index].tangent(0.0)?),
            (end_loops[index].clone(), -pieces[index].tangent(1.0)?),
        ];
        add_surface_face(&mut body, surface, loops, Some(index))?;
    }
    for (surface, edges, along) in extensions {
        // Seen from its first edge an extension face lies ahead along the
        // run it extends: run a's from its section, run b's from the mitre.
        add_surface_face(&mut body, surface, vec![(edges, along)], None)?;
    }
    for (section, rim, sense) in caps {
        let plane = Plane::orthonormal(section.centre.to_array(), section.x.to_array(), (section.normal * sense).to_array())?;
        let rim_edge = body.edges.get(rim)?;
        let rim_point = Vec3::from(body.curves.get(rim_edge.curve)?.point_at(0.5 * (rim_edge.start_parameter + rim_edge.end_parameter)));
        add_surface_face(&mut body, Surface::Plane(plane), vec![(vec![rim], section.centre - rim_point)], None)?;
    }
    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

/// The outward normal of a tube, extension or cap surface at a point on it.
fn outward_normal(surface: &Surface, point: Vec3) -> Vec3 {
    match surface {
        Surface::Plane(plane) => plane.normal().map(Vec3::from).unwrap_or(Vec3::Z),
        Surface::Cylinder(cylinder) => {
            let axis = cylinder.base.normal().map(Vec3::from).unwrap_or(Vec3::Z);
            let offset = point - Vec3::from(cylinder.base.origin);
            (offset - axis * offset.dot(axis)).normalize().unwrap_or(Vec3::X)
        }
        Surface::Torus(torus) => {
            let axis = torus.frame.normal().map(Vec3::from).unwrap_or(Vec3::Z);
            let offset = point - Vec3::from(torus.frame.origin);
            let flat = (offset - axis * offset.dot(axis)).normalize().unwrap_or(Vec3::X);
            let ring = Vec3::from(torus.frame.origin) + flat * torus.major_radius;
            (point - ring).normalize().unwrap_or(Vec3::X)
        }
        _ => Vec3::Z,
    }
}

/// A closed path of straight runs whose transported section comes back
/// turned (a non-planar polyline). The runs keep their own transported
/// sections, as the reference modeler closes such a path: the first section,
/// carried onto the last run's direction, extends the last run up to the
/// mitre plane, and the first run reaches back to whichever comes first of
/// the mitre plane and the last run's lateral faces that look along it.
/// Polygon profiles only: the pieces are joined by planar Booleans.
fn closed_turned_polyline(
    pieces: &[Piece],
    first: Frame,
    profile_plane_wires: &[Vec<Curve>],
    wires: &[Wire],
    options: SweepOptions,
    radius: f64,
) -> Option<Body> {
    if pieces.len() < 3
        || !pieces.iter().all(|piece| matches!(piece, Piece::Line(_, _)))
        || !profile_plane_wires.iter().flatten().all(|curve| matches!(curve, Curve::Line(_)))
        || options.twist.abs() > 1e-12 || (options.scale - 1.0).abs() > 1e-12
    {
        return None;
    }
    let start_tangent = pieces.first()?.tangent(0.0)?;
    let end_tangent = pieces.last()?.tangent(1.0)?;
    let corner = pieces.first()?.point(0.0);
    // Split a middle run in two, so that neither half overlaps itself.
    let Piece::Line(from, to) = pieces[1] else { return None };
    let middle = from.lerp(to, 0.5);
    let normal = (start_tangent + end_tangent).normalize()?;
    let reach = radius * 4.0 / start_tangent.dot(normal).abs().max(0.05);
    // The first run starts well back past the corner and is cut below.
    let Piece::Line(_, first_end) = pieces[0] else { return None };
    let back = Frame { origin: first.origin - start_tangent * reach, ..first };
    let head = [Piece::Line(corner - start_tangent * reach, first_end), Piece::Line(from, middle)];
    let mut tail = vec![Piece::Line(middle, to)];
    tail.extend(pieces[2..].iter().cloned());
    let length = |part: &[Piece]| part.iter().map(Piece::length).sum::<f64>();
    let head_patches = transported_patches(&head, back, options, length(&head), radius, false)?;
    let tail_patches = transported_patches(&tail, head_patches.last()?[3], options, length(&tail), radius, false)?;
    let last = tail_patches.last()?[3];
    let carried = moved(last, end_tangent, start_tangent, corner, corner)?;
    // A section that comes back unturned closes as an ordinary mitre.
    if frame_error(carried, first, radius) <= radius * 1e-9 {
        return None;
    }
    let mut runs = Vec::new();
    for patches in [&head_patches, &tail_patches] {
        let outward = regular_transport(wires, patches, false)?;
        let mut part = build_body(wires, patches, false, false, outward)?;
        analytic_ruled_faces(&mut part);
        runs.push(part);
    }
    let tail_body = runs.pop()?;
    let head_body = runs.pop()?;
    let mitre = Plane::orthonormal(corner.to_array(), start_tangent.cross(end_tangent).normalize()?.to_array(), normal.to_array())?;
    let mut parts = vec![tail_body, super::slice::slice_by_plane(&head_body, mitre).ok()??.positive];
    // The first section carried onto the last run's direction, swept on
    // past the corner and cut at the mitre plane.
    let onto_last = moved(first, start_tangent, end_tangent, corner, corner)?;
    let ahead = super::sweep::extrude_region(onto_last.plane(), profile_plane_wires, (end_tangent * reach).to_array())?;
    parts.push(super::slice::slice_by_plane(&ahead, mitre).ok()??.negative);
    // The first run also reaches back to every lateral face of the last run
    // that looks along it.
    let vertices: Vec<[f64; 2]> = profile_plane_wires.iter().flatten().map(|curve| curve.point_at(0.0)).collect();
    let sum = vertices.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0], a[1] + p[1]]);
    let centre = last.point([sum[0] / vertices.len() as f64, sum[1] / vertices.len() as f64]);
    for curve in profile_plane_wires.iter().flatten() {
        let a = last.point(curve.point_at(0.0));
        let Some(mut outward) = (last.point(curve.point_at(1.0)) - a).cross(end_tangent).normalize() else { continue };
        if outward.dot(a - centre) < 0.0 { outward = -outward; }
        // A face nearly along the first run would be cut far behind the
        // corner, at a reach set only by the fallback above.
        if outward.dot(start_tangent) <= 1e-3 { continue; }
        let face = Plane::orthonormal(a.to_array(), end_tangent.to_array(), outward.to_array())?;
        if let Some(cut) = super::slice::slice_by_plane(&head_body, face).ok()? { parts.push(cut.positive); }
    }
    let mut parts = parts.into_iter();
    let mut body = parts.next()?;
    for part in parts {
        let tolerance = super::operation_tolerance(&[&body, &part]);
        body = super::combine(body, part, super::Operation::Union, tolerance).ok()?;
    }
    analytic_ruled_faces(&mut body);
    body.validate().is_empty().then_some(body)
}

/// One run of a planar path, in the path plane: a segment, or an arc from a
/// start angle through a signed sweep.
#[derive(Clone, Copy)]
enum Run2 {
    Segment([f64; 2], [f64; 2]),
    Turn { centre: [f64; 2], radius: f64, start: f64, sweep: f64 },
}

impl Run2 {
    fn point(self, t: f64) -> [f64; 2] {
        match self {
            Self::Segment(a, b) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            Self::Turn { centre, radius, start, sweep } => {
                let angle = start + sweep * t;
                [centre[0] + radius * angle.cos(), centre[1] + radius * angle.sin()]
            }
        }
    }

    fn tangent(self, t: f64) -> [f64; 2] {
        match self {
            Self::Segment(a, b) => {
                let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                [(b[0] - a[0]) / length, (b[1] - a[1]) / length]
            }
            Self::Turn { start, sweep, .. } => {
                let angle = start + sweep * t;
                let sign = sweep.signum();
                [-angle.sin() * sign, angle.cos() * sign]
            }
        }
    }

    /// The run offset to its left by `distance`.
    fn offset(self, distance: f64) -> Option<Self> {
        Some(match self {
            Self::Segment(a, b) => {
                let t = self.tangent(0.0);
                let left = [-t[1] * distance, t[0] * distance];
                Self::Segment([a[0] + left[0], a[1] + left[1]], [b[0] + left[0], b[1] + left[1]])
            }
            Self::Turn { centre, radius, start, sweep } => {
                let radius = radius - distance * sweep.signum();
                if radius <= 1e-9 {
                    return None;
                }
                Self::Turn { centre, radius, start, sweep }
            }
        })
    }

    /// The run cut to begin (`at_start`) or end at a point on it.
    fn trimmed(self, point: [f64; 2], at_start: bool) -> Option<Self> {
        Some(match self {
            Self::Segment(a, b) => if at_start { Self::Segment(point, b) } else { Self::Segment(a, point) },
            Self::Turn { centre, radius, start, sweep } => {
                let angle = (point[1] - centre[1]).atan2(point[0] - centre[0]);
                // The turn from the start to the point, along the sweep.
                let mut along = ((angle - start) * sweep.signum()).rem_euclid(TAU);
                if along > sweep.abs() + PI {
                    along -= TAU;
                }
                let along = along * sweep.signum();
                if at_start {
                    Self::Turn { centre, radius, start: start + along, sweep: sweep - along }
                } else {
                    Self::Turn { centre, radius, start, sweep: along }
                }
            }
        })
        .filter(|run| run.length() > 1e-9)
    }

    fn length(self) -> f64 {
        match self {
            Self::Segment(a, b) => (b[0] - a[0]).hypot(b[1] - a[1]),
            Self::Turn { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    fn curve(self) -> Curve {
        match self {
            Self::Segment(start, end) => Curve::Line(Line { start, end }),
            Self::Turn { centre, radius, start, sweep } => {
                let (start_angle, end_angle) = if sweep > 0.0 { (start, start + sweep) } else { (start + sweep, start) };
                Curve::Arc(Arc { centre, radius, start_angle, end_angle })
            }
        }
    }

    /// Where the whole line or circle of this run meets that of another,
    /// nearest to `near`.
    fn meet(self, other: Self, near: [f64; 2]) -> Option<[f64; 2]> {
        let mut points = Vec::new();
        match (self, other) {
            (Self::Segment(a, b), Self::Segment(c, d)) => {
                let (r, s) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
                let denominator = r[0] * s[1] - r[1] * s[0];
                if denominator.abs() <= 1e-12 * r[0].hypot(r[1]) * s[0].hypot(s[1]) {
                    return None;
                }
                let t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / denominator;
                points.push([a[0] + r[0] * t, a[1] + r[1] * t]);
            }
            (Self::Segment(a, b), Self::Turn { centre, radius, .. })
            | (Self::Turn { centre, radius, .. }, Self::Segment(a, b)) => {
                let r = [b[0] - a[0], b[1] - a[1]];
                let f = [a[0] - centre[0], a[1] - centre[1]];
                let qa = r[0] * r[0] + r[1] * r[1];
                let qb = 2.0 * (f[0] * r[0] + f[1] * r[1]);
                let qc = f[0] * f[0] + f[1] * f[1] - radius * radius;
                let discriminant = qb * qb - 4.0 * qa * qc;
                if discriminant < 0.0 {
                    return None;
                }
                for sign in [-1.0, 1.0] {
                    let t = (-qb + sign * discriminant.sqrt()) / (2.0 * qa);
                    points.push([a[0] + r[0] * t, a[1] + r[1] * t]);
                }
            }
            (Self::Turn { centre: c0, radius: r0, .. }, Self::Turn { centre: c1, radius: r1, .. }) => {
                let d = (c1[0] - c0[0]).hypot(c1[1] - c0[1]);
                if d <= 1e-12 || d > r0 + r1 || d < (r0 - r1).abs() {
                    return None;
                }
                let along = (d * d + r0 * r0 - r1 * r1) / (2.0 * d);
                let across = (r0 * r0 - along * along).max(0.0).sqrt();
                let u = [(c1[0] - c0[0]) / d, (c1[1] - c0[1]) / d];
                for sign in [-1.0, 1.0] {
                    points.push([c0[0] + u[0] * along - u[1] * across * sign, c0[1] + u[1] * along + u[0] * across * sign]);
                }
            }
        }
        let distance = |x: &[f64; 2]| (x[0] - near[0]).hypot(x[1] - near[1]);
        points.into_iter().min_by(|p, q| distance(p).total_cmp(&distance(q)))
    }
}

/// One side of a band: the runs offset by `distance`, joined at corners as
/// the reference modeler joins a rectangular section: on the outer side
/// each run carries on straight to where the two meet, on the inner side
/// the runs are cut where they cross.
fn band_side(runs: &[Run2], distance: f64, closed: bool) -> Option<Vec<Run2>> {
    let count = runs.len();
    let mut sides = runs.iter().map(|run| run.offset(distance)).collect::<Option<Vec<_>>>()?;
    let mut before = vec![None; count];
    let mut after = vec![None; count];
    let joints = if closed { count } else { count - 1 };
    for index in 0..joints {
        let next = (index + 1) % count;
        let (t1, t2) = (runs[index].tangent(1.0), runs[next].tangent(0.0));
        let turn = t1[0] * t2[1] - t1[1] * t2[0];
        let smooth = turn.abs() <= 1e-9 && t1[0] * t2[0] + t1[1] * t2[1] > 0.0;
        if smooth || distance.abs() <= 1e-12 {
            continue;
        }
        let (end, start) = (sides[index].point(1.0), sides[next].point(0.0));
        if distance * turn > 0.0 {
            let point = sides[index].meet(sides[next], end)?;
            sides[index] = sides[index].trimmed(point, false)?;
            sides[next] = sides[next].trimmed(point, true)?;
        } else {
            let reach = Run2::Segment(end, [end[0] + t1[0], end[1] + t1[1]]);
            let back = Run2::Segment(start, [start[0] + t2[0], start[1] + t2[1]]);
            let point = reach.meet(back, end)?;
            match sides[index] {
                Run2::Segment(a, _) => sides[index] = Run2::Segment(a, point),
                _ => after[index] = Some(Run2::Segment(end, point)),
            }
            match sides[next] {
                Run2::Segment(_, b) => sides[next] = Run2::Segment(point, b),
                _ => before[next] = Some(Run2::Segment(point, start)),
            }
        }
    }
    let mut result = Vec::new();
    for index in 0..count {
        result.extend(before[index]);
        result.push(sides[index]);
        result.extend(after[index]);
    }
    result.retain(|run| run.length() > 1e-9);
    Some(result)
}

/// A rectangular section square to a planar path whose corners meet arcs:
/// the solid is the band its in-plane edges sweep, extruded across the
/// plane, with exact planar and cylindrical faces.
fn planar_band(pieces: &[Piece], first: Frame, profile: &[Vec<Curve>], closed: bool) -> Option<Body> {
    let (plane, runs) = planar_runs(pieces)?;
    let normal = Vec3::from(plane.normal()?);
    let origin = Vec3::from(plane.origin);
    // Only where a corner meets an arc; the general sweep handles the rest.
    if !corner_on_turn(&runs, closed) {
        return None;
    }
    // The placed section: a rectangle with sides across and along the plane.
    let start = pieces.first()?.point(0.0);
    let left = normal.cross(pieces.first()?.tangent(0.0)?).normalize()?;
    let [wire] = profile else { return None };
    if wire.len() != 4 {
        return None;
    }
    let (mut across, mut height) = (Vec::new(), Vec::new());
    for curve in wire {
        let Curve::Line(line) = curve else { return None };
        let (a, b) = (first.point(line.start), first.point(line.end));
        let direction = (b - a).normalize()?;
        if direction.dot(normal).abs() < 1.0 - 1e-9 && direction.dot(left).abs() < 1.0 - 1e-9 {
            return None;
        }
        across.push((a - start).dot(left));
        height.push((a - start).dot(normal));
    }
    let bounds = |values: &[f64]| values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let ((right, outer), (bottom, top)) = (bounds(&across), bounds(&height));
    if outer - right <= 1e-9 || top - bottom <= 1e-9 {
        return None;
    }
    let one = band_side(&runs, outer, closed)?;
    let other = band_side(&runs, right, closed)?;
    let mut loops = Vec::new();
    if closed {
        let area = |ring: &[Run2]| ring.iter().map(|run| {
            let (a, b) = (run.point(0.0), run.point(1.0));
            a[0] * b[1] - a[1] * b[0]
        }).sum::<f64>().abs();
        let (outside, inside) = if area(&one) >= area(&other) { (&one, &other) } else { (&other, &one) };
        loops.push(outside.iter().map(|run| run.curve()).collect::<Vec<_>>());
        loops.push(inside.iter().map(|run| run.curve()).collect::<Vec<_>>());
    } else {
        let mut ring = one.iter().map(|run| run.curve()).collect::<Vec<_>>();
        ring.push(Curve::Line(Line { start: one.last()?.point(1.0), end: other.last()?.point(1.0) }));
        ring.extend(other.iter().rev().map(|run| run.curve()));
        ring.push(Curve::Line(Line { start: other.first()?.point(0.0), end: one.first()?.point(0.0) }));
        loops.push(ring);
    }
    let base = Plane::from_axes((origin + normal * bottom).to_array(), plane.x_axis, plane.y_axis);
    let body = super::sweep::extrude_region(base, &loops, (normal * (top - bottom)).to_array())?;
    body.validate().is_empty().then_some(body)
}

/// The runs of a path lying in one plane, in that plane's coordinates.
fn planar_runs(pieces: &[Piece]) -> Option<(Plane, Vec<Run2>)> {
    let plane = pieces.iter().find_map(|piece| match piece {
        Piece::Planar(plane, _, _) => Some(*plane),
        _ => None,
    })?;
    let normal = Vec3::from(plane.normal()?);
    let origin = Vec3::from(plane.origin);
    let runs = pieces.iter().map(|piece| Some(match piece {
        Piece::Line(a, b) => {
            let scale = a.length().max(b.length()).max(1.0) * 1e-9;
            if (*a - origin).dot(normal).abs() > scale || (*b - origin).dot(normal).abs() > scale {
                return None;
            }
            Run2::Segment(plane.project(a.to_array())?, plane.project(b.to_array())?)
        }
        Piece::Planar(other, _, _) => {
            if Vec3::from(other.normal()?).dot(normal).abs() < 1.0 - 1e-12
                || (Vec3::from(other.origin) - origin).dot(normal).abs() > 1e-9 * origin.length().max(1.0)
            {
                return None;
            }
            // A circular arc, however it is represented: the circle through
            // its ends and middle, which every other sample must lie on.
            let at = |t: f64| plane.project(piece.point(t).to_array());
            let (p0, pm, p1) = (at(0.0)?, at(0.5)?, at(1.0)?);
            let (b, c) = ([pm[0] - p0[0], pm[1] - p0[1]], [p1[0] - p0[0], p1[1] - p0[1]]);
            let denominator = 2.0 * (b[0] * c[1] - b[1] * c[0]);
            if denominator.abs() <= 1e-12 * piece.length() * piece.length() {
                return None;
            }
            let (bb, cc) = (b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
            let centre = [p0[0] + (c[1] * bb - b[1] * cc) / denominator, p0[1] + (b[0] * cc - c[0] * bb) / denominator];
            let radius = (p0[0] - centre[0]).hypot(p0[1] - centre[1]);
            for t in [0.125, 0.25, 0.375, 0.625, 0.75, 0.875] {
                let q = at(t)?;
                if ((q[0] - centre[0]).hypot(q[1] - centre[1]) - radius).abs() > 1e-9 * radius.max(1.0) {
                    return None;
                }
            }
            let angle = |q: [f64; 2]| (q[1] - centre[1]).atan2(q[0] - centre[0]);
            let sign = denominator.signum();
            let sweep = sign * ((angle(p1) - angle(p0)) * sign).rem_euclid(TAU);
            if sweep.abs() <= 1e-12 {
                return None;
            }
            Run2::Turn { centre, radius, start: angle(p0), sweep }
        }
        _ => return None,
    })).collect::<Option<Vec<_>>>()?;
    Some((plane, runs))
}

/// Whether a planar path has a corner where an arc takes part.
fn corner_on_turn(runs: &[Run2], closed: bool) -> bool {
    let count = runs.len();
    let joints = if closed { count } else { count - 1 };
    (0..joints).any(|index| {
        let next = (index + 1) % count;
        let (t1, t2) = (runs[index].tangent(1.0), runs[next].tangent(0.0));
        t1[0] * t2[0] + t1[1] * t2[1] < 1.0 - 1e-9
            && (matches!(runs[index], Run2::Turn { .. }) || matches!(runs[next], Run2::Turn { .. }))
    })
}

/// Where the band boundary at one offset turns from one piece to the next.
#[derive(Clone, Copy, PartialEq)]
enum Bound {
    /// A straight joint: a smooth joint, an arc's end, or the mitre.
    Joint,
    /// Where the pieces either side of an inner corner cross.
    Cross(usize),
}

/// The band boundary at a left offset of a planar path, with each corner
/// joined the way `inner` says (the side the offset lies on, one per
/// joint): the pieces in order, with the bounds at both ends of each (one
/// more bound than pieces). A closed path's boundary starts and ends at its
/// closing joint. Pieces may have no length (an offset on the path itself);
/// the structure depends only on `inner`, so every offset with the same
/// sides shares it.
fn polygon_rail(runs: &[Run2], distance: f64, inner: &[bool], closed: bool) -> Option<(Vec<Run2>, Vec<Bound>)> {
    let count = runs.len();
    let mut sides = runs.iter().map(|run| run.offset(distance)).collect::<Option<Vec<_>>>()?;
    let mut before = vec![None; count];
    let mut after = vec![None; count];
    let mut joints = vec![Bound::Joint; count];
    let on_path = distance.abs() <= 1e-12;
    for index in 0..inner.len() {
        let next = (index + 1) % count;
        let (t1, t2) = (runs[index].tangent(1.0), runs[next].tangent(0.0));
        let turn = t1[0] * t2[1] - t1[1] * t2[0];
        if turn.abs() <= 1e-9 && t1[0] * t2[0] + t1[1] * t2[1] > 0.0 {
            continue;
        }
        let (end, start) = (sides[index].point(1.0), sides[next].point(0.0));
        if inner[index] {
            let point = if on_path { end } else { sides[index].meet(sides[next], end)? };
            sides[index] = sides[index].trimmed(point, false)?;
            sides[next] = sides[next].trimmed(point, true)?;
            joints[index] = Bound::Cross(index);
        } else {
            let reach = Run2::Segment(end, [end[0] + t1[0], end[1] + t1[1]]);
            let back = Run2::Segment(start, [start[0] + t2[0], start[1] + t2[1]]);
            let point = if on_path { end } else { reach.meet(back, end)? };
            match sides[index] {
                Run2::Segment(a, _) => sides[index] = Run2::Segment(a, point),
                _ => after[index] = Some(Run2::Segment(end, point)),
            }
            match sides[next] {
                Run2::Segment(_, b) => sides[next] = Run2::Segment(point, b),
                _ => before[next] = Some(Run2::Segment(point, start)),
            }
        }
    }
    let ends = if closed { joints[count - 1] } else { Bound::Joint };
    let (mut pieces, mut bounds) = (Vec::new(), vec![ends]);
    for index in 0..count {
        if let Some(run) = before[index] {
            pieces.push(run);
            bounds.push(Bound::Joint);
        }
        pieces.push(sides[index]);
        if let Some(run) = after[index] {
            bounds.push(Bound::Joint);
            pieces.push(run);
        }
        bounds.push(if index + 1 < count { joints[index] } else { ends });
    }
    Some((pieces, bounds))
}

/// A straight-sided section square to a planar path, open or closed, whose
/// corners meet arcs. As the reference modeler joins such a corner, every
/// slice of the solid along the path's plane is the band its section
/// sweeps there: on the outer side of a corner each run carries on straight
/// to where the two meet, on the inner side the runs are cut where they
/// cross. Each section edge sweeps planes along straight runs and cones
/// round arcs; the inner crossings are the exact intersections of those
/// faces. The section may be concave: every edge sweeps its own faces.
fn planar_polygon_band(pieces: &[Piece], first: Frame, profile: &[Vec<Curve>], closed: bool) -> Option<Body> {
    let (plane, runs) = planar_runs(pieces)?;
    if !plane.is_orthonormal() || runs.len() < 2 || !corner_on_turn(&runs, closed) {
        return None;
    }
    let normal = Vec3::from(plane.normal()?);
    let start = pieces.first()?.point(0.0);
    let start_tangent = pieces.first()?.tangent(0.0)?;
    if Vec3::from(first.plane().normal()?).cross(start_tangent).length() > 1e-9 {
        return None;
    }
    let left = normal.cross(start_tangent).normalize()?;
    // The section in (left offset, height above the plane), counter-clockwise.
    let [wire] = profile else { return None };
    let curves = expanded(wire)?;
    let senses = chain_senses(&curves)?;
    if !chain_closed(&curves, &senses) {
        return None;
    }
    let mut section = Vec::new();
    for (curve, forward) in curves.iter().zip(&senses) {
        let Curve::Line(_) = curve else { return None };
        let point = first.point(curve.point_at(if *forward { 0.0 } else { 1.0 })) - start;
        section.push([point.dot(left), point.dot(normal)]);
    }
    let area = (0..section.len()).map(|index| {
        let (a, b) = (section[index], section[(index + 1) % section.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum::<f64>();
    if area.abs() <= 1e-12 {
        return None;
    }
    if area < 0.0 {
        section.reverse();
    }
    let size = section.iter().map(|p| p[0].hypot(p[1])).fold(0.0_f64, f64::max).max(1e-9);
    let count = section.len();
    for index in 0..count {
        let (a, b, c) = (section[index], section[(index + 1) % count], section[(index + 2) % count]);
        if ((b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])).abs() <= 1e-12 * size * size {
            return None;
        }
    }
    // Edges that pass over the path are split there: each side of it joins
    // the corners its own way.
    let mut vertices: Vec<[f64; 2]> = Vec::new();
    for index in 0..count {
        let (a, b) = (section[index], section[(index + 1) % count]);
        vertices.push(a);
        if a[0] * b[0] < 0.0 && a[0].abs() > 1e-12 * size && b[0].abs() > 1e-12 * size {
            let t = a[0] / (a[0] - b[0]);
            vertices.push([0.0, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let count = vertices.len();
    let corners = if closed { runs.len() } else { runs.len() - 1 };
    let following = |corner: usize| (corner + 1) % runs.len();
    let turns = (0..corners).map(|index| {
        let (t1, t2) = (runs[index].tangent(1.0), runs[following(index)].tangent(0.0));
        t1[0] * t2[1] - t1[1] * t2[0]
    }).collect::<Vec<_>>();
    let inner_for = |side: f64| turns.iter().map(|turn| side * turn > 0.0).collect::<Vec<_>>();
    let side_of = |a: [f64; 2], b: [f64; 2]| if a[0] + b[0] >= 0.0 { 1.0 } else { -1.0 };
    let lift = |point: [f64; 2], height: f64| Vec3::from(plane.point_at(point)) + normal * height;
    let tolerance = size.max(1.0) * 1e-9;

    let mut body = Body::new();
    let lump = body.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
    let shell = body.shells.insert(Shell { faces: Vec::new(), owner: lump, provenance: Provenance::Synthesized });
    // Rail vertices and edges, by section vertex: shared by its two strips.
    let mut rail_vertices: Vec<Vec<(Vec3, VertexKey)>> = vec![Vec::new(); count];
    let mut rail_edges: Vec<Vec<(VertexKey, VertexKey, EdgeKey)>> = vec![Vec::new(); count];
    let mut vertex_at = |body: &mut Body, rail: usize, point: Vec3| -> VertexKey {
        if let Some((_, key)) = rail_vertices[rail].iter().find(|(at, _)| at.distance(point) <= tolerance) {
            return *key;
        }
        let key = body.vertices.insert(Vertex { point: point.to_array(), provenance: Provenance::Synthesized });
        rail_vertices[rail].push((point, key));
        key
    };
    let new_edge = |body: &mut Body, curve: Curve3, from: VertexKey, to: VertexKey, low: f64, high: f64| -> EdgeKey {
        let curve = body.curves.insert(curve);
        body.edges.insert(Edge { curve, start_parameter: low, end_parameter: high, start: from, end: to, coedges: Vec::new(), provenance: Provenance::Synthesized })
    };
    let line_edge = |body: &mut Body, from: (VertexKey, Vec3), to: (VertexKey, Vec3)| -> Option<EdgeKey> {
        let direction = (to.1 - from.1).normalize()?;
        Some(new_edge(body, Curve3::Line(super::geometry::Line3 { origin: from.1.to_array(), direction: direction.to_array() }), from.0, to.0, 0.0, to.1.distance(from.1)))
    };
    // A piece of a rail at a height, as an edge between its end vertices.
    let piece_edge = |body: &mut Body, piece: Run2, height: f64, from: (VertexKey, Vec3), to: (VertexKey, Vec3)| -> Option<EdgeKey> {
        match piece {
            Run2::Segment(_, _) => line_edge(body, from, to),
            Run2::Turn { centre, radius, start, sweep } => {
                let circle = Curve3::Circle(Circle3 { plane: Plane::orthonormal(lift(centre, height).to_array(), plane.x_axis, normal.to_array())?, radius });
                Some(if sweep > 0.0 {
                    new_edge(body, circle, from.0, to.0, start, start + sweep)
                } else {
                    new_edge(body, circle, to.0, from.0, start + sweep, start)
                })
            }
        }
    };

    struct Strip { faces: Vec<(Surface, Vec<EdgeKey>, Vec3, Vec3)> }
    let mut start_cap = Vec::new();
    let mut end_cap = Vec::new();
    // NURBS crossing edges, with the points and tangents they were fitted
    // through, for their traces on each face.
    let mut crossings: Vec<(EdgeKey, Vec<Vec3>, Vec3, Vec3)> = Vec::new();
    let mut strips = Vec::new();
    for index in 0..count {
        let (k, l) = (index, (index + 1) % count);
        let (a, b) = (vertices[k], vertices[l]);
        let inner = inner_for(side_of(a, b));
        let (pieces_a, bounds) = polygon_rail(&runs, a[0], &inner, closed)?;
        let (pieces_b, _) = polygon_rail(&runs, b[0], &inner, closed)?;
        let slots = pieces_a.len();
        // The points of both rails at every bound, the ends included.
        let points = |rail: &[Run2]| {
            let mut points = vec![rail[0].point(0.0)];
            points.extend(rail.iter().map(|piece| piece.point(1.0)));
            points
        };
        let (points_a, points_b) = (points(&pieces_a), points(&pieces_b));
        let at_a = points_a.iter().map(|p| lift(*p, a[1])).collect::<Vec<_>>();
        let at_b = points_b.iter().map(|p| lift(*p, b[1])).collect::<Vec<_>>();
        let keys_a = at_a.iter().map(|p| vertex_at(&mut body, k, *p)).collect::<Vec<_>>();
        let keys_b = at_b.iter().map(|p| vertex_at(&mut body, l, *p)).collect::<Vec<_>>();
        // Rail edges, made once per section vertex.
        let mut rail_edge = |body: &mut Body, rail: usize, piece: Run2, height: f64, from: (VertexKey, Vec3), to: (VertexKey, Vec3)| -> Option<Option<EdgeKey>> {
            if from.0 == to.0 {
                return Some(None);
            }
            if let Some((_, _, key)) = rail_edges[rail].iter().find(|(s, e, _)| (*s, *e) == (from.0, to.0) || (*s, *e) == (to.0, from.0)) {
                return Some(Some(*key));
            }
            let key = piece_edge(body, piece, height, from, to)?;
            rail_edges[rail].push((from.0, to.0, key));
            Some(Some(key))
        };
        // Edges across the strip at every bound.
        let mut across: Vec<EdgeKey> = Vec::new();
        for bound in 0..=slots {
            // A closed rail ends where it starts.
            if closed && bound == slots {
                across.push(across[0]);
                continue;
            }
            if bound > 0 && keys_a[bound] == keys_a[bound - 1] && keys_b[bound] == keys_b[bound - 1] {
                across.push(*across.last()?);
                continue;
            }
            let kind = bounds[bound];
            let from = (keys_a[bound], at_a[bound]);
            let to = (keys_b[bound], at_b[bound]);
            let both_straight = matches!(kind, Bound::Cross(corner)
                if matches!((runs[corner], runs[following(corner)]), (Run2::Segment(..), Run2::Segment(..))));
            let edge = match kind {
                Bound::Cross(corner) if !both_straight => {
                    // The two pieces' crossing as the offset moves along the
                    // section edge: on both faces exactly, fitted closely.
                    let cross = |share: f64| -> Option<Vec3> {
                        let distance = a[0] + (b[0] - a[0]) * share;
                        let height = a[1] + (b[1] - a[1]) * share;
                        let (one, two) = (runs[corner].offset(distance)?, runs[following(corner)].offset(distance)?);
                        let end = one.point(1.0);
                        let point = if distance.abs() <= 1e-12 { end } else { one.meet(two, end)? };
                        Some(lift(point, height))
                    };
                    let slope = |share: f64, at: Vec3, inward: f64| -> Option<Vec3> {
                        let h = 1e-4 * inward;
                        let (near, far) = (cross(share + h)?, cross(share + 2.0 * h)?);
                        Some((at * -3.0 + near * 4.0 - far) / (2.0 * h))
                    };
                    let mut fitted = None;
                    for samples in [8usize, 16, 32, 64, 128, 256, 512, 1024] {
                        let mut points = vec![from.1];
                        for step in 1..samples {
                            points.push(cross(step as f64 / samples as f64)?);
                        }
                        points.push(to.1);
                        let start_tangent = slope(0.0, from.1, 1.0)? / samples as f64;
                        let end_tangent = slope(1.0, to.1, -1.0)? / samples as f64;
                        let curve = NurbsCurve3::interpolate_fit(&points.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
                            Some(start_tangent.to_array()), Some(end_tangent.to_array()), ocs_geom::space::Parameterization::Uniform)?;
                        let mut worst: f64 = 0.0;
                        for step in 0..samples {
                            let exact = cross((step as f64 + 0.5) / samples as f64)?;
                            worst = worst.max(Vec3::from(curve.point_at((step as f64 + 0.5) / samples as f64)).distance(exact));
                        }
                        if worst <= 1e-8 * size.max(1.0) {
                            fitted = Some((curve, points, start_tangent, end_tangent));
                            break;
                        }
                    }
                    let (curve, points, start_tangent, end_tangent) = fitted?;
                    let (low, high) = curve.domain();
                    let key = new_edge(&mut body, Curve3::Nurbs(curve), from.0, to.0, low, high);
                    crossings.push((key, points, start_tangent, end_tangent));
                    key
                }
                _ => line_edge(&mut body, from, to)?,
            };
            across.push(edge);
        }
        start_cap.push(across[0]);
        end_cap.push(across[slots]);
        // A face per piece: plane, cylinder or cone through both rails.
        let outward2 = [b[1] - a[1], a[0] - b[0]];
        let mut faces = Vec::new();
        for slot in 0..slots {
            let (piece_a, piece_b) = (pieces_a[slot], pieces_b[slot]);
            let edge_a = rail_edge(&mut body, k, piece_a, a[1], (keys_a[slot], at_a[slot]), (keys_a[slot + 1], at_a[slot + 1]))?;
            let edge_b = rail_edge(&mut body, l, piece_b, b[1], (keys_b[slot], at_b[slot]), (keys_b[slot + 1], at_b[slot + 1]))?;
            if edge_a.is_none() && edge_b.is_none() {
                continue;
            }
            let mut edges = vec![across[slot], across[slot + 1]];
            edges.extend(edge_a);
            edges.extend(edge_b);
            // Outward at the middle of the piece on rail a (or b).
            let (piece, height) = if edge_a.is_some() { (piece_a, a[1]) } else { (piece_b, b[1]) };
            let middle = piece.point(0.5);
            let left2 = match piece {
                Run2::Segment(..) => { let t = piece.tangent(0.5); [-t[1], t[0]] }
                Run2::Turn { centre, sweep, .. } => {
                    let (dx, dy) = (centre[0] - middle[0], centre[1] - middle[1]);
                    let length = dx.hypot(dy);
                    [dx / length * sweep.signum(), dy / length * sweep.signum()]
                }
            };
            let left3 = Vec3::from(plane.vector_at(left2));
            let outward = (left3 * outward2[0] + normal * outward2[1]).normalize()?;
            let surface = match (piece_a, piece_b) {
                (Run2::Turn { centre, radius: ra, .. }, Run2::Turn { radius: rb, .. }) => {
                    let base = |height: f64| Plane::orthonormal(lift(centre, height).to_array(), plane.x_axis, normal.to_array());
                    if (a[1] - b[1]).abs() <= tolerance {
                        Surface::Plane(base(a[1])?)
                    } else if (ra - rb).abs() <= tolerance {
                        Surface::Cylinder(super::geometry::Cylinder { base: base(a[1])?, radius: ra })
                    } else {
                        Surface::Cone(super::geometry::Cone { base: base(a[1])?, radius: ra, half_angle: ((ra - rb) / (b[1] - a[1])).atan() })
                    }
                }
                _ => {
                    let direction = Vec3::from(plane.vector_at(piece.tangent(0.5))).normalize()?;
                    let point = lift(middle, height);
                    Surface::Plane(Plane::orthonormal(point.to_array(), direction.to_array(), direction.cross(outward).cross(direction).normalize()?.to_array())?)
                }
            };
            faces.push((surface, edges, outward, lift(middle, height)));
        }
        strips.push(Strip { faces });
    }
    // Faces: each loop runs counter-clockwise seen from outside.
    let chain = |body: &Body, edges: &[EdgeKey]| -> Option<Vec<(EdgeKey, bool)>> {
        let mut edges = edges.to_vec();
        edges.dedup();
        let first = body.edges.get(edges[0])?;
        let mut at = first.end;
        let mut circuit = vec![(edges[0], true)];
        let mut rest = edges[1..].to_vec();
        while !rest.is_empty() {
            let index = rest.iter().position(|key| body.edges.get(*key).is_some_and(|node| node.start == at || node.end == at))?;
            let key = rest.remove(index);
            let node = body.edges.get(key)?;
            if node.start == at { circuit.push((key, true)); at = node.end; } else { circuit.push((key, false)); at = node.start; }
        }
        (at == first.start).then_some(circuit)
    };
    let newell = |body: &Body, circuit: &[(EdgeKey, bool)]| -> Option<Vec3> {
        let mut points = Vec::new();
        for (key, forward) in circuit {
            let edge = body.edges.get(*key)?;
            let curve = body.curves.get(edge.curve)?;
            let mut samples = (0..8).map(|step| Vec3::from(curve.point_at(edge.start_parameter
                + (edge.end_parameter - edge.start_parameter) * step as f64 / 8.0))).collect::<Vec<_>>();
            if !forward {
                samples.push(Vec3::from(curve.point_at(edge.end_parameter)));
                samples.reverse();
                samples.pop();
            }
            points.extend(samples);
        }
        let mut sum = Vec3::ZERO;
        for index in 0..points.len() {
            sum = sum + points[index].cross(points[(index + 1) % points.len()]);
        }
        Some(sum)
    };
    let place = |body: &mut Body, surface: Surface, edges: &[EdgeKey], outward: Vec3, at: Vec3| -> Option<FaceKey> {
        let mut circuit = chain(body, edges)?;
        if newell(body, &circuit)?.dot(outward) < 0.0 {
            circuit = circuit.into_iter().rev().map(|(edge, forward)| (edge, !forward)).collect();
        }
        let (u, v) = surface.parameters_at(at.to_array())?;
        let forward = Vec3::from(surface.normal_at(u, v)?).dot(outward) > 0.0;
        let key = body.surfaces.insert(surface);
        let face = add_face(body, shell, key, forward);
        add_loop(body, face, &circuit, None)?;
        Some(face)
    };
    for strip in strips {
        for (surface, edges, outward, at) in strip.faces {
            place(&mut body, surface, &edges, outward, at)?;
        }
    }
    let end_tangent = pieces.last()?.tangent(1.0)?;
    let end = pieces.last()?.point(1.0);
    let caps = if closed { Vec::new() } else { vec![(start_cap, -start_tangent, start), (end_cap, end_tangent, end)] };
    for (edges, outward, at) in caps {
        let x = normal.cross(outward).normalize()?;
        let surface = Surface::Plane(Plane::orthonormal(at.to_array(), x.to_array(), outward.to_array())?);
        place(&mut body, surface, &edges, outward, at)?;
    }
    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    // Faces either side of a section edge split over the path share one
    // surface: join them, and the straight edges left meeting in line there.
    analytic_ruled_faces(&mut body)?;
    while let Some(vertex) = { let found = body.vertices.keys().find(|vertex| super::boolean::collinear_pair(&body, *vertex).is_some()); found } {
        super::boolean::join_edges(&mut body, vertex)?;
    }
    // Each crossing's trace on the faces it bounds, in their final surfaces.
    for (key, points, start_tangent, end_tangent) in crossings {
        let Some(edge) = body.edges.get(key).cloned() else { continue };
        for coedge in edge.coedges {
            let node = body.coedges.get(coedge)?;
            let face = body.loops.get(node.owner)?.owner;
            let surface = body.surfaces.get(body.faces.get(face)?.surface)?.clone();
            let periodic = !matches!(surface, Surface::Plane(_));
            let mut uv: Vec<[f64; 2]> = Vec::with_capacity(points.len());
            for point in &points {
                let (mut u, v) = surface.parameters_at(point.to_array())?;
                if let (true, Some(&[pu, _])) = (periodic, uv.last()) {
                    u += TAU * ((pu - u) / TAU).round();
                }
                uv.push([u, v]);
            }
            let uv_tangent = |point: Vec3, tangent: Vec3, at: [f64; 2]| -> Option<[f64; 2]> {
                let h = 1e-6 / tangent.length().max(1e-12);
                let (mut u, v) = surface.parameters_at((point + tangent * h).to_array())?;
                if periodic { u += TAU * ((at[0] - u) / TAU).round(); }
                Some([(u - at[0]) / h, (v - at[1]) / h])
            };
            let start_uv = uv_tangent(points[0], start_tangent, uv[0])?;
            let end_uv = uv_tangent(points[points.len() - 1], end_tangent, uv[uv.len() - 1])?;
            let trace = NurbsCurve::interpolate(&uv, Some(start_uv), Some(end_uv), ocs_geom::space::Parameterization::Uniform)?;
            let (start_knot, end_knot) = trace.domain();
            let knots = trace.knots().iter().map(|k| edge.start_parameter + (k - start_knot) / (end_knot - start_knot) * (edge.end_parameter - edge.start_parameter)).collect();
            let trace = NurbsCurve::new_strict(trace.degree(), trace.control_points().to_vec(), knots, trace.weights().to_vec())?;
            let forward = body.coedges.get(coedge)?.forward;
            body.coedges.get_mut(coedge)?.pcurve = Some(Curve::Nurbs(if forward { trace } else { trace.reversed() }));
        }
    }
    body.validate().is_empty().then_some(body)
}

fn rotationally_invariant(wires: &[Vec<Curve>]) -> bool {
    wires.iter().all(|wire| {
        let mut radius: Option<f64> = None;
        let mut angle = 0.0;
        for piece in wire {
            let Curve::Arc(arc) = piece else { return false; };
            if arc.centre[0].hypot(arc.centre[1]) > arc.radius.abs().max(1.0) * 1e-10 { return false; }
            if radius.is_some_and(|value| (value - arc.radius).abs() > value.abs().max(1.0) * 1e-10) { return false; }
            radius = Some(arc.radius);
            angle += arc.sweep();
        }
        (angle - TAU).abs() <= 1e-9
    })
}

const GAUSS: [(f64, f64); 5] = [
    (-0.906179845938664, 0.236926885056189),
    (-0.538469310105683, 0.478628670499366),
    (0.0, 0.568888888888889),
    (0.538469310105683, 0.478628670499366),
    (0.906179845938664, 0.236926885056189),
];

struct Wire {
    source: Vec<Curve>,
    curves: Vec<RationalCurve2>,
    closed: bool,
}

/// A planar path's curves traversed the other way, in chain order.
pub fn reversed_path_curves(curves: &[Curve]) -> Option<Vec<Curve>> {
    let curves = expanded(curves)?;
    let senses = chain_senses(&curves)?;
    curves.iter().zip(senses).rev().map(|(curve, forward)| {
        let mut rational = RationalCurve2::from_curve(curve)?;
        // Path pieces are evaluated over [0, 1].
        let (low, high) = (*rational.knots.first()?, *rational.knots.last()?);
        rational.knots = rational.knots.iter().map(|k| (k - low) / (high - low)).collect();
        let rational = if forward { rational.reversed() } else { rational };
        Some(Curve::Nurbs(NurbsCurve::new_strict(rational.degree, rational.points, rational.knots, rational.weights)?))
    }).collect()
}

fn expanded(curves: &[Curve]) -> Option<Vec<Curve>> {
    let mut result = Vec::new();
    for curve in curves {
        match curve {
            Curve::Polyline(polyline) => {
                for (index, segment) in curve.segments().into_iter().enumerate() {
                    if near2(polyline.vertices[index].position, segment.point_at(0.0)) {
                        result.push(segment);
                    } else {
                        // Clockwise bulges are represented by a reversed
                        // CCW arc. Preserve the entity's directed start even
                        // when it contains only this one segment.
                        let rational = RationalCurve2::from_curve(&segment)?.reversed();
                        result.push(Curve::Nurbs(NurbsCurve::new_strict(rational.degree,
                            rational.points, rational.knots, rational.weights)?));
                    }
                }
            }
            Curve::Circle(circle) => {
                result.push(Curve::Arc(Arc { centre: circle.centre, radius: circle.radius,
                    start_angle: 0.0, end_angle: TAU }));
            }
            Curve::Ray(_) | Curve::XLine(_) => return None,
            _ => result.push(curve.clone()),
        }
    }
    (!result.is_empty()).then_some(result)
}

fn chain_senses(pieces: &[Curve]) -> Option<Vec<bool>> {
    [true, false].into_iter().find_map(|first| {
        let mut senses = vec![first];
        let mut point = pieces.first()?.point_at(if first { 1.0 } else { 0.0 });
        for piece in &pieces[1..] {
            let forward = near2(point, piece.point_at(0.0));
            if !forward && !near2(point, piece.point_at(1.0)) { return None; }
            senses.push(forward);
            point = piece.point_at(if forward { 1.0 } else { 0.0 });
        }
        Some(senses)
    })
}

fn near2(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8
        + a.into_iter().chain(b).map(f64::abs).fold(1.0_f64, f64::max) * f64::EPSILON * 64.0
}

fn chain_closed(pieces: &[Curve], senses: &[bool]) -> bool {
    near2(pieces[0].point_at(if senses[0] { 0.0 } else { 1.0 }),
        pieces.last().unwrap().point_at(if *senses.last().unwrap() { 1.0 } else { 0.0 }))
}

fn prepare_wires(wires: &[Vec<Curve>]) -> Option<Vec<Wire>> {
    if wires.is_empty() { return None; }
    wires.iter().enumerate().map(|(index, source)| {
        let source = expanded(source)?;
        let senses = chain_senses(&source)?;
        let closed = chain_closed(&source, &senses);
        let origin = source[0].point_at(0.0);
        let shift = Transform::translation([-origin[0], -origin[1]]);
        let area = source.iter().zip(&senses)
            .map(|(piece, forward)| Some(piece.transformed(&shift)?.chain_area()
                * if *forward { 1.0 } else { -1.0 }))
            .collect::<Option<Vec<_>>>()?.into_iter().sum::<f64>();
        if closed && (!area.is_finite() || area.abs() <= 1e-14) { return None; }
        let mut curves = source.iter().zip(senses).map(|(piece, forward)| {
            let curve = RationalCurve2::from_curve(piece)?;
            Some(if forward { curve } else { curve.reversed() })
        }).collect::<Option<Vec<_>>>()?;
        if closed && (area > 0.0) != (index == 0) {
            curves = curves.into_iter().rev().map(|curve| curve.reversed()).collect();
        }
        Some(Wire { source, curves, closed })
    }).collect()
}

#[derive(Clone)]
enum Piece {
    Line(Vec3, Vec3),
    Planar(Plane, Curve, bool),
    Spline(NurbsCurve3, f64, f64),
}

impl Piece {
    fn point(&self, t: f64) -> Vec3 {
        Vec3::from(match self {
            Self::Line(a, b) => a.lerp(*b, t).to_array(),
            Self::Planar(plane, curve, forward) =>
                plane.point_at(curve.point_at(if *forward { t } else { 1.0 - t })),
            Self::Spline(curve, a, b) => curve.point_at_knot(a + (b - a) * t),
        })
    }

    fn tangent(&self, t: f64) -> Option<Vec3> {
        let direction = match self {
            Self::Line(a, b) => *b - *a,
            Self::Planar(plane, curve, forward) => {
                Vec3::from(plane.vector_at(curve.tangent_at(if *forward { t } else { 1.0 - t })))
                    * if *forward { 1.0 } else { -1.0 }
            }
            Self::Spline(_, _, _) => self.spline_derivative(t),
        };
        direction.is_finite().then_some(())?;
        if direction.length() > 1e-12 { return direction.normalize(); }
        let delta = self.point((t + 1e-6).min(1.0)) - self.point((t - 1e-6).max(0.0));
        delta.normalize()
    }

    fn speed(&self, t: f64) -> f64 {
        match self {
            Self::Line(a, b) => a.distance(*b),
            Self::Planar(plane, curve, forward) => Vec3::from(plane.vector_at(
                curve.tangent_at(if *forward { t } else { 1.0 - t }))).length(),
            Self::Spline(_, _, _) => self.spline_derivative(t).length(),
        }
    }

    fn spline_derivative(&self, t: f64) -> Vec3 {
        // The analytic derivative, held just inside this knot span so a
        // deliberately sharp polyline corner at its ends is not rounded. A
        // difference quotient here was too rough for the sections' fits.
        let Self::Spline(curve, a, b) = self else { return Vec3::ZERO };
        let inset = (b - a) * 1e-10;
        Vec3::from(curve.derivative_at_knot((a + (b - a) * t).clamp(a + inset, b - inset))) * (b - a)
    }

    fn length_to(&self, end: f64) -> f64 {
        if let Self::Line(a, b) = self { return a.distance(*b) * end; }
        if let Self::Planar(plane, curve, _) = self {
            if let Curve::Line(line) = curve {
                return Vec3::from(plane.vector_at(line.direction())).length() * end;
            }
            if matches!(curve, Curve::Arc(_)) && plane.is_orthonormal() {
                return curve.length() * end;
            }
        }
        // Integrate knot span by knot span: the speed of a spline is only
        // piecewise smooth, and a panel across a knot loses accuracy.
        let mut breaks = vec![0.0];
        if let Self::Planar(_, Curve::Nurbs(curve), forward) = self {
            let (start, stop) = curve.domain();
            breaks.extend(curve.knots().iter()
                .map(|knot| (knot - start) / (stop - start))
                .map(|t| if *forward { t } else { 1.0 - t })
                .filter(|t| *t > 1e-12 && *t < end - 1e-12));
            breaks.sort_by(f64::total_cmp);
        }
        breaks.push(end);
        let panels = if breaks.len() > 2 { 8 } else { 16 };
        breaks.windows(2).map(|range| {
            let width = range[1] - range[0];
            (0..panels).map(|panel| GAUSS.into_iter().map(|(node, weight)| {
                let t = range[0] + width * (panel as f64 + 0.5 + node * 0.5) / panels as f64;
                self.speed(t) * weight * width / (2 * panels) as f64
            }).sum::<f64>()).sum::<f64>()
        }).sum()
    }

    fn length(&self) -> f64 { self.length_to(1.0) }

    /// The parameter `distance` along the piece from its start.
    fn parameter_at_length(&self, distance: f64) -> f64 {
        let length = self.length();
        let mut t = (distance / length).clamp(0.0, 1.0);
        if matches!(self, Self::Line(..)) { return t; }
        for _ in 0..8 {
            let speed = self.speed(t);
            if !(speed > 1e-12) { break; }
            t = (t - (self.length_to(t) - distance) / speed).clamp(0.0, 1.0);
        }
        t
    }
}

fn path_pieces(path: SweepPath<'_>) -> Option<Vec<Piece>> {
    let pieces = match path {
        SweepPath::Planar { plane, curves } => {
            plane.normal()?;
            let curves = expanded(curves)?;
            let senses = chain_senses(&curves)?;
            curves.into_iter().zip(senses).map(|(curve, forward)| match curve {
                Curve::Line(line) => Piece::Line(
                    Vec3::from(plane.point_at(if forward { line.start } else { line.end })),
                    Vec3::from(plane.point_at(if forward { line.end } else { line.start }))),
                _ => Piece::Planar(plane, curve, forward),
            }).collect::<Vec<_>>()
        }
        SweepPath::Polyline3d { points, closed } => {
            if points.len() < 2 || !points.iter().flatten().all(|v| v.is_finite()) { return None; }
            let mut pieces = points.windows(2).map(|pair|
                Piece::Line(Vec3::from(pair[0]), Vec3::from(pair[1]))).collect::<Vec<_>>();
            if closed && Vec3::from(points[0]).distance(Vec3::from(*points.last()?)) > 1e-9 {
                pieces.push(Piece::Line(Vec3::from(*points.last()?), Vec3::from(points[0])));
            }
            pieces
        }
        SweepPath::Nurbs3(curve) => {
            let (a, b) = curve.domain();
            let mut knots = curve.knots().iter().copied().filter(|k| *k >= a && *k <= b).collect::<Vec<_>>();
            knots.dedup_by(|a, b| (*a - *b).abs() <= 1e-14);
            knots.windows(2).map(|span| Piece::Spline(curve.clone(), span[0], span[1])).collect()
        }
    };
    if pieces.is_empty() || pieces.iter().any(|piece| !piece.length().is_finite() || piece.length() <= 1e-12) {
        return None;
    }
    Some(pieces)
}

#[derive(Clone, Copy)]
struct Frame { origin: Vec3, x: Vec3, y: Vec3 }

impl Frame {
    fn plane(self) -> Plane { Plane::from_axes(self.origin.to_array(), self.x.to_array(), self.y.to_array()) }
    fn point(self, p: [f64; 2]) -> Vec3 { self.origin + self.x * p[0] + self.y * p[1] }
    fn plus(self, other: Self) -> Self { Self { origin: self.origin + other.origin, x: self.x + other.x, y: self.y + other.y } }
    fn minus(self, other: Self) -> Self { self.plus(other.times(-1.0)) }
    fn times(self, t: f64) -> Self { Self { origin: self.origin * t, x: self.x * t, y: self.y * t } }
    fn lerp(self, other: Self, t: f64) -> Self { self.plus(other.minus(self).times(t)) }
}

fn rotate(value: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    value * cos + axis.cross(value) * sin + axis * axis.dot(value) * (1.0 - cos)
}

fn transport(value: Vec3, from: Vec3, to: Vec3) -> Option<Vec3> {
    let cross = from.cross(to);
    let sin = cross.length();
    let cos = from.dot(to).clamp(-1.0, 1.0);
    if sin <= 1e-12 {
        if cos >= 0.0 { return Some(value); }
        let axis = if from.x.abs() < 0.8 { from.cross(Vec3::X) } else { from.cross(Vec3::Y) }.normalize()?;
        return Some(rotate(value, axis, PI));
    }
    Some(rotate(value, cross / sin, sin.atan2(cos)))
}

fn moved(frame: Frame, from: Vec3, to: Vec3, old_point: Vec3, point: Vec3) -> Option<Frame> {
    Some(Frame { origin: point + transport(frame.origin - old_point, from, to)?,
        x: transport(frame.x, from, to)?, y: transport(frame.y, from, to)? })
}

fn curved_transport(frame: Frame, piece: &Piece, a: f64, b: f64, bank: bool) -> Option<Frame> {
    let from = piece.tangent(a)?;
    let to = piece.tangent(b)?;
    let point = piece.point(b);
    let mut result = moved(frame, from, to, piece.point(a), point)?;
    if bank {
        let binormal = |t: f64| -> Option<Vec3> {
            let start = piece.tangent((t - 1e-3).max(0.0))?;
            let end = piece.tangent((t + 1e-3).min(1.0))?;
            let cross = start.cross(end);
            (cross.length() > 1e-9).then(|| cross.normalize()).flatten()
        };
        if let (Some(previous), Some(next)) = (binormal(a), binormal(b)) {
            let previous = transport(previous, from, to)?;
            let roll = to.dot(previous.cross(next)).atan2(previous.dot(next));
            result = Frame { origin: point + rotate(result.origin - point, to, roll),
                x: rotate(result.x, to, roll), y: rotate(result.y, to, roll) };
        }
    }
    Some(result)
}

fn miter(frame: Frame, point: Vec3, tangent: Vec3, other: Vec3) -> Option<Frame> {
    let normal = (tangent + other).normalize()?;
    let dot = normal.dot(tangent);
    if dot <= 1e-6 { return None; }
    let cut = |v: Vec3| v - tangent * (v.dot(normal) / dot);
    Some(Frame { origin: point + cut(frame.origin - point), x: cut(frame.x), y: cut(frame.y) })
}

fn twist_frame(frame: Frame, point: Vec3, angle: f64, scale: f64) -> Option<Frame> {
    let normal = frame.x.cross(frame.y).normalize()?;
    Some(Frame { origin: point + rotate(frame.origin - point, normal, angle) * scale,
        x: rotate(frame.x, normal, angle) * scale, y: rotate(frame.y, normal, angle) * scale })
}

/// Cubic Bezier control frames of one path patch, with their weights: one
/// for transport fits, rational for an exact turn about a circular run.
/// `span` is the patch's share of its run's parameter, which spaces the
/// run's knots so that Hermite patches join smoothly.
#[derive(Clone, Copy)]
struct Patch { frames: [Frame; 4], weights: [f64; 4], span: f64 }

impl std::ops::Index<usize> for Patch {
    type Output = Frame;
    fn index(&self, index: usize) -> &Frame { &self.frames[index] }
}

impl std::ops::IndexMut<usize> for Patch {
    fn index_mut(&mut self, index: usize) -> &mut Frame { &mut self.frames[index] }
}

/// The circle a planar piece runs round: centre, unit axis (turning the
/// piece positively) and the angle it turns through.
fn circular_run(piece: &Piece) -> Option<(Vec3, Vec3, f64)> {
    let Piece::Planar(plane, curve, _) = piece else { return None };
    if !matches!(curve, Curve::Arc(_) | Curve::Nurbs(_)) { return None; }
    let (a, b, c) = (piece.point(0.0), piece.point(0.5), piece.point(1.0));
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    if normal.length() <= 1e-12 * ab.length().max(ac.length()).max(1.0).powi(2) { return None; }
    let centre = a + (normal.cross(ab) * ac.dot(ac) + ac.cross(normal) * ab.dot(ab)) * (0.5 / normal.dot(normal));
    let radius = a.distance(centre);
    let tolerance = radius.max(1.0) * 1e-10;
    if (0..=16).any(|step| (piece.point(step as f64 / 16.0).distance(centre) - radius).abs() > tolerance) {
        return None;
    }
    let plane_normal = Vec3::from(plane.normal()?);
    let axis = if (a - centre).cross(piece.tangent(0.0)?).dot(plane_normal) >= 0.0 { plane_normal } else { -plane_normal };
    let (from, to) = (a - centre, c - centre);
    let mut angle = axis.dot(from.cross(to)).atan2(from.dot(to));
    // The middle point decides whether the run goes the long way round.
    if angle <= 0.0 || axis.dot((b - centre).cross(to)) < 0.0 { angle += TAU; }
    if angle > TAU { angle -= TAU; }
    (angle > 1e-9).then_some((centre, axis, angle))
}

/// Exact rational patches turning `start` about the axis through `centre`,
/// each a quarter turn at most.
fn turning_patches(start: Frame, centre: Vec3, axis: Vec3, angle: f64, result: &mut Vec<Patch>) {
    let spans = (angle / (PI / 2.0) - 1e-9).ceil().max(1.0) as usize;
    let step = angle / spans as f64;
    let turned = |frame: Frame, by: f64| Frame {
        origin: centre + rotate(frame.origin - centre, axis, by),
        x: rotate(frame.x, axis, by),
        y: rotate(frame.y, axis, by),
    };
    // The middle control of a rational quadratic arc lies 1/cos(step/2) out
    // from the axis; along the axis nothing moves.
    let cosine = (step * 0.5).cos();
    let spread = |value: Vec3| axis * axis.dot(value) + (value - axis * axis.dot(value)) * (1.0 / cosine);
    for span in 0..spans {
        let first = turned(start, step * span as f64);
        let last = turned(start, step * (span + 1) as f64);
        let middle = turned(start, step * (span as f64 + 0.5));
        let corner = Frame {
            origin: centre + spread(middle.origin - centre),
            x: spread(middle.x),
            y: spread(middle.y),
        };
        // Degree elevation of weights (1, cos, 1).
        let inner = 1.0 + 2.0 * cosine;
        result.push(Patch {
            frames: [
                first,
                first.plus(corner.times(2.0 * cosine)).times(1.0 / inner),
                corner.times(2.0 * cosine).plus(last).times(1.0 / inner),
                last,
            ],
            weights: [1.0, inner / 3.0, inner / 3.0, 1.0],
            span: step,
        });
    }
}

fn transported_patches(
    pieces: &[Piece], first: Frame, options: SweepOptions,
    total: f64, radius: f64, closed: bool,
) -> Option<Vec<Patch>> {
    Some(transported_runs(pieces, first, options, total, radius, closed, None)?.0)
}

/// The rail the reference modeller lays beside a scaled sweep's path: the
/// first section's x axis, `length` long, carried along the path, twisted
/// by `twist` and scaled like the profile. It approximates that rail by
/// cubic Hermite spans and takes each section's turn and scale from the
/// approximation, not from the exact laws, so its solids carry the
/// approximation's error in turn and scale; reproduce it.
#[derive(Clone, Copy)]
struct RailLaw { length: f64, twist: f64 }

/// One piecewise cubic Hermite rail over a run parameter.
struct Rail { knots: Vec<f64>, points: Vec<Vec3>, slopes: Vec<Vec3> }

impl Rail {
    fn at(&self, u: f64) -> Vec3 {
        let span = self.knots.partition_point(|knot| *knot <= u).clamp(1, self.knots.len() - 1) - 1;
        let (a, b) = (self.knots[span], self.knots[span + 1]);
        let h = b - a;
        let t = ((u - a) / h).clamp(0.0, 1.0);
        let s = 1.0 - t;
        let (p0, p3) = (self.points[span], self.points[span + 1]);
        let p1 = p0 + self.slopes[span] * (h / 3.0);
        let p2 = p3 - self.slopes[span + 1] * (h / 3.0);
        p0 * (s * s * s) + p1 * (3.0 * s * s * t) + p2 * (3.0 * s * t * t) + p3 * (t * t * t)
    }
}

/// Target and limit of the reference rail fit: a span is accepted at an
/// error up to `RAIL_TOLERANCE`; a refused span is resized by the fourth
/// root of `RAIL_TARGET` over its error, as the reference sizes its spans.
const RAIL_TOLERANCE: f64 = 1e-3;
const RAIL_TARGET: f64 = 8.20691e-4;

/// The reference rail fit over `0..=length`: Hermite spans marched from the
/// start. The first trial span covers `first` (an eighth of a turn of the
/// path and twist together), later trials twice the span before. A refused
/// span is resized until it fits; a first span fitting well inside the
/// target is retried from the whole rest instead. Measured on straight and
/// circular paths, the knots this gives match the reference's to about
/// 0.1 % of a span; along a spline they drift further (the reference's own
/// trial rule there is not known).
fn fit_rail(exact: &impl Fn(f64) -> Option<Vec3>, length: f64, first: f64) -> Option<Rail> {
    let step = length * 1e-6;
    let slope = |u: f64| -> Option<Vec3> {
        Some(if u - step < 0.0 {
            (exact(u)? * -3.0 + exact(u + step)? * 4.0 - exact(u + 2.0 * step)?) / (2.0 * step)
        } else if u + step > length {
            (exact(u)? * 3.0 - exact(u - step)? * 4.0 + exact(u - 2.0 * step)?) / (2.0 * step)
        } else {
            (exact(u + step)? - exact(u - step)?) / (2.0 * step)
        })
    };
    let error = |a: f64, b: f64, pa: Vec3, da: Vec3| -> Option<(f64, Vec3, Vec3)> {
        let (pb, db) = (exact(b)?, slope(b)?);
        let rail = Rail { knots: vec![a, b], points: vec![pa, pb], slopes: vec![da, db] };
        let mut worst = 0.0_f64;
        for sample in 1..32 {
            let u = a + (b - a) * sample as f64 / 32.0;
            worst = worst.max(rail.at(u).distance(exact(u)?));
        }
        Some((worst, pb, db))
    };
    let mut rail = Rail { knots: vec![0.0], points: vec![exact(0.0)?], slopes: vec![slope(0.0)?] };
    let mut at = 0.0;
    while at < length * (1.0 - 1e-12) {
        if rail.knots.len() > 4096 { return None; }
        let rest = length - at;
        let (pa, da) = (*rail.points.last()?, *rail.slopes.last()?);
        let fit = |trial: f64| -> Option<(f64, f64, Vec3, Vec3)> {
            let mut span = trial.min(rest);
            for _ in 0..64 {
                let (worst, pb, db) = error(at, at + span, pa, da)?;
                if !worst.is_finite() { return None; }
                if worst <= RAIL_TOLERANCE { return Some((span, worst, pb, db)); }
                span = (span * (RAIL_TARGET / worst).powf(0.25)).min(rest);
            }
            None
        };
        let previous = rail.knots.len().checked_sub(2).map(|index| at - rail.knots[index]);
        let mut found = fit(previous.map_or(first, |span| 2.0 * span))?;
        if rail.knots.len() == 1 && found.1 < RAIL_TARGET && found.0 < rest {
            found = fit(rest)?;
        }
        let (span, _, pb, db) = found;
        at = if span >= rest { length } else { at + span };
        rail.knots.push(at);
        rail.points.push(pb);
        rail.slopes.push(db);
    }
    Some(rail)
}

/// The transported patches and how many of them each path piece has.
/// With `rail`, sections take their turn and scale from the reference's
/// approximated rail (see `RailLaw`) instead of the exact laws.
fn transported_runs(
    pieces: &[Piece], first: Frame, options: SweepOptions,
    total: f64, radius: f64, closed: bool, rail: Option<RailLaw>,
) -> Option<(Vec<Patch>, Vec<usize>)> {
    let mut frame = first;
    let mut previous_point = pieces[0].point(0.0);
    let mut previous_tangent = pieces[0].tangent(0.0)?;
    // Transport through small angular increments, also on nonplanar splines.
    let mut walks = Vec::new();
    for piece in pieces {
        let tangent = piece.tangent(0.0)?;
        if previous_tangent.dot(tangent) <= -1.0 + 1e-10 { return None; }
        frame = moved(frame, previous_tangent, tangent, previous_point, piece.point(0.0))?;
        let mut parameters = vec![0.0];
        divide_path(piece, 0.0, 1.0, 0, &mut parameters)?;
        let mut frames = vec![frame];
        for span in parameters.windows(2) {
            frame = curved_transport(frame, piece, span[0], span[1], options.bank)?;
            frames.push(frame);
        }
        previous_point = piece.point(1.0);
        previous_tangent = piece.tangent(1.0)?;
        walks.push((parameters, frames));
    }
    let closure_roll = if closed {
        let last = moved(frame, previous_tangent, pieces[0].tangent(0.0)?, previous_point, pieces[0].point(0.0))?;
        let axis = pieces[0].tangent(0.0)?;
        let a = (last.x - axis * last.x.dot(axis)).normalize()?;
        let b = (first.x - axis * first.x.dot(axis)).normalize()?;
        axis.dot(a.cross(b)).atan2(a.dot(b))
    } else { 0.0 };
    // The twist and scale laws run linearly in the path's own parameter:
    // the knot parameter along a spline, as the reference measures it, and
    // the length along anything else.
    let spline = pieces.iter().all(|piece| matches!(piece, Piece::Spline(..)));
    let widths = pieces.iter().map(|piece| match piece {
        Piece::Spline(_, a, b) if spline => b - a,
        _ => piece.length(),
    }).collect::<Vec<_>>();
    let lengths = pieces.iter().map(Piece::length).collect::<Vec<_>>();
    let width_total = widths.iter().sum::<f64>();
    let before = |values: &[f64], index: usize| values[..index].iter().sum::<f64>();
    let fraction = |index: usize, t: f64| if spline {
        (before(&widths, index) + widths[index] * t) / width_total
    } else {
        (before(&lengths, index) + pieces[index].length_to(t)) / total
    };
    let raw = |index: usize, t: f64| -> Option<Frame> {
        let (parameters, frames) = &walks[index];
        let slot = parameters.partition_point(|parameter| *parameter <= t).saturating_sub(1).min(parameters.len() - 2);
        curved_transport(frames[slot], &pieces[index], parameters[slot], t, options.bank)
    };
    // The reference fits one rail per path piece, one along a whole spline.
    let rail = rail.filter(|_| (options.scale - 1.0).abs() > 1e-12);
    let groups = if spline { vec![(0, pieces.len())] } else { (0..pieces.len()).map(|index| (index, index + 1)).collect() };
    let mut rails = Vec::new();
    if let Some(law) = rail {
        for &(from, to) in &groups {
            let length = widths[from..to].iter().sum::<f64>();
            // A spline's rail runs in its knot parameter, a piece's rail
            // in its length.
            let locate = |u: f64| -> (usize, f64) {
                if !spline { return (from, pieces[from].parameter_at_length(u)); }
                let mut index = from;
                let mut start = 0.0;
                while index + 1 < to && u > start + widths[index] {
                    start += widths[index];
                    index += 1;
                }
                (index, ((u - start) / widths[index]).clamp(0.0, 1.0))
            };
            let exact = |u: f64| -> Option<Vec3> {
                let (index, t) = locate(u);
                let section = raw(index, t)?;
                let normal = section.x.cross(section.y).normalize()?;
                let f = fraction(index, t);
                Some(pieces[index].point(t) + rotate(section.x.normalize()?, normal, law.twist * f)
                    * (law.length * (1.0 + (options.scale - 1.0) * f)))
            };
            // An eighth of a turn of the path and the twist together.
            let mut turning = law.twist.abs() * length / width_total;
            for index in from..to {
                for pair in walks[index].0.windows(2) {
                    turning += pieces[index].tangent(pair[0])?.dot(pieces[index].tangent(pair[1])?).clamp(-1.0, 1.0).acos();
                }
            }
            let first = if turning > PI / 4.0 { length * PI / 4.0 / turning } else { length };
            rails.push(fit_rail(&exact, length, first)?);
        }
    }
    let rail_at = |index: usize, t: f64| -> Option<(usize, f64)> {
        let group = groups.iter().position(|&(from, to)| index >= from && index < to)?;
        if !spline { return Some((group, pieces[index].length_to(t))); }
        Some((group, before(&widths, index) - before(&widths, groups[group].0) + widths[index] * t))
    };
    let mut patches = Vec::new();
    let mut runs = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let evaluate_raw = |t: f64| -> Option<Frame> {
            let section = raw(index, t)?;
            let point = piece.point(t);
            let f = fraction(index, t);
            let Some(law) = rail else {
                return twist_frame(section, point, options.twist * f + closure_roll * f,
                    1.0 + (options.scale - 1.0) * f);
            };
            let (group, u) = rail_at(index, t)?;
            let normal = section.x.cross(section.y).normalize()?;
            let x = section.x.normalize()?;
            let offset = rails[group].at(u) - point;
            let offset = offset - normal * offset.dot(normal);
            // A round profile ignores the turn (the twist was dropped).
            let turn = if options.twist.abs() > 1e-12 { normal.dot(x.cross(offset)).atan2(x.dot(offset)) } else { 0.0 };
            twist_frame(section, point, turn, offset.length() / law.length)
        };
        let raw_start = evaluate_raw(0.0)?;
        let raw_end = evaluate_raw(1.0)?;
        let previous = if index > 0 { Some(&pieces[index - 1]) } else if closed { pieces.last() } else { None };
        let next = if index + 1 < pieces.len() { Some(&pieces[index + 1]) } else if closed { pieces.first() } else { None };
        let cut_start = if let Some(previous) = previous {
            miter(raw_start, piece.point(0.0), piece.tangent(0.0)?, previous.tangent(1.0)?)?
        } else { raw_start };
        let cut_end = if let Some(next) = next {
            miter(raw_end, piece.point(1.0), piece.tangent(1.0)?, next.tangent(0.0)?)?
        } else { raw_end };
        let delta_start = cut_start.minus(raw_start);
        let delta_end = cut_end.minus(raw_end);
        let evaluate = |t: f64| -> Option<Frame> {
            Some(evaluate_raw(t)?.plus(delta_start.lerp(delta_end, t)))
        };
        // An untwisted, unscaled run round a circle that meets its neighbours
        // without a corner is an exact turn: keep it exact rather than fitted.
        let delta = delta_start.origin.length() + delta_start.x.length() + delta_start.y.length()
            + delta_end.origin.length() + delta_end.x.length() + delta_end.y.length();
        let exact = (options.twist.abs() + closure_roll.abs() <= 1e-12
            && (options.scale - 1.0).abs() <= 1e-12 && delta <= 1e-12)
            .then(|| circular_run(piece)).flatten();
        if let Some((centre, axis, angle)) = exact {
            turning_patches(evaluate(0.0)?, centre, axis, angle, &mut patches);
        } else {
            // The rail's knots, where the sections are only once smooth, cut
            // the fit.
            let mut cuts = vec![0.0, 1.0];
            if rail.is_some() {
                let (group, offset) = rail_at(index, 0.0)?;
                cuts.extend(rails[group].knots.iter()
                    .map(|knot| if spline { (knot - offset) / widths[index] } else { piece.parameter_at_length(*knot) })
                    .filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9));
                cuts.sort_by(f64::total_cmp);
            }
            hermite_fit(&evaluate, &cuts, widths[index], radius, radius * 1e-7, &mut patches)?;
        }
        runs.push(patches.len() - runs.iter().sum::<usize>());
    }
    // Shared topology requires exactly the same section on both sides of a
    // path corner. Minimal transport plus the bisector cut gives that map;
    // reject banking singularities rather than hiding a discontinuity.
    for index in 1..patches.len() {
        if frame_error(patches[index - 1][3], patches[index][0], radius) > total.max(radius) * 1e-6 { return None; }
        patches[index][0] = patches[index - 1][3];
    }
    if closed {
        let start = patches.first()?[0];
        if frame_error(patches.last()?[3], start, radius) > total.max(radius) * 1e-6 { return None; }
        patches.last_mut()?[3] = start;
    }
    // The knot spans of one spline are a single run.
    if spline {
        runs = vec![patches.len()];
    }
    Some((patches, runs))
}

/// A failed fit span is cut at this share of its length and the near part
/// tried again, the reference modeller's own split.
const FIT_SPLIT: f64 = 0.506107193;

/// Cubic Hermite patches through `evaluate` over each interval of `cuts`
/// (piece parameters), with its exact end derivatives so neighbouring
/// patches join smoothly. Spans are marched from each interval's start:
/// the whole rest is tried, and a span over `tolerance` is cut at
/// `FIT_SPLIT` until it fits. `width` is the run parameter per unit of `t`.
fn hermite_fit(
    evaluate: &impl Fn(f64) -> Option<Frame>, cuts: &[f64], width: f64,
    radius: f64, tolerance: f64, result: &mut Vec<Patch>,
) -> Option<()> {
    const STEP: f64 = 1e-5;
    let slope = |t: f64| -> Option<Frame> {
        Some(if t - STEP < 0.0 {
            evaluate(t)?.times(-3.0).plus(evaluate(t + STEP)?.times(4.0)).minus(evaluate(t + 2.0 * STEP)?).times(0.5 / STEP)
        } else if t + STEP > 1.0 {
            evaluate(t)?.times(3.0).minus(evaluate(t - STEP)?.times(4.0)).plus(evaluate(t - 2.0 * STEP)?).times(0.5 / STEP)
        } else {
            evaluate(t + STEP)?.minus(evaluate(t - STEP)?).times(0.5 / STEP)
        })
    };
    for interval in cuts.windows(2) {
        let (mut a, end) = (interval[0], interval[1]);
        let (mut fa, mut da) = (evaluate(a)?, slope(a)?);
        while a < end {
            let mut b = end;
            loop {
                let (fb, db) = (evaluate(b)?, slope(b)?);
                let h = b - a;
                let patch = Patch {
                    frames: [fa, fa.plus(da.times(h / 3.0)), fb.minus(db.times(h / 3.0)), fb],
                    weights: [1.0; 4],
                    span: width * h,
                };
                let mut error = 0.0_f64;
                for t in [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875] {
                    error = error.max(frame_error(bezier(&patch, t), evaluate(a + h * t)?, radius));
                }
                if !error.is_finite() { return None; }
                if error <= tolerance {
                    if result.len() >= 8192 { return None; }
                    result.push(patch);
                    (a, fa, da) = (b, fb, db);
                    break;
                }
                if h <= (end - interval[0]) * 1e-9 { return None; }
                b = a + h * FIT_SPLIT;
            }
        }
    }
    Some(())
}

fn divide_path(piece: &Piece, a: f64, b: f64, depth: usize, result: &mut Vec<f64>) -> Option<()> {
    let middle = (a + b) * 0.5;
    let ta = piece.tangent(a)?;
    let tb = piece.tangent(b)?;
    let tm = piece.tangent(middle)?;
    let chord = piece.point(a).distance(piece.point(b));
    let broken = piece.point(a).distance(piece.point(middle)) + piece.point(middle).distance(piece.point(b));
    if ta.dot(tm) < 0.99875 || tm.dot(tb) < 0.99875 || broken - chord > broken.max(1.0) * 0.0001 {
        if depth >= 16 || result.len() >= 8192 { return None; }
        divide_path(piece, a, middle, depth + 1, result)?;
        divide_path(piece, middle, b, depth + 1, result)?;
    } else {
        result.push(b);
    }
    Some(())
}

fn bezier(p: &Patch, t: f64) -> Frame {
    let s = 1.0 - t;
    let basis = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
    let weight = (0..4).map(|i| basis[i] * p.weights[i]).sum::<f64>();
    (0..4).fold(p[0].times(0.0), |sum, i| sum.plus(p[i].times(basis[i] * p.weights[i] / weight)))
}

fn bezier_derivative(p: &Patch, t: f64) -> Frame {
    let s = 1.0 - t;
    let basis = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
    let slope = [-3.0 * s * s, 3.0 * s * s - 6.0 * s * t, 6.0 * s * t - 3.0 * t * t, 3.0 * t * t];
    let weight = (0..4).map(|i| basis[i] * p.weights[i]).sum::<f64>();
    let weight_slope = (0..4).map(|i| slope[i] * p.weights[i]).sum::<f64>();
    let point = bezier(p, t);
    (0..4).fold(point.times(-weight_slope / weight), |sum, i| {
        sum.plus(p[i].times(slope[i] * p.weights[i] / weight))
    })
}

/// A locally folded transport is not a regular swept body, even when its
/// shared-edge topology happens to be manifold. Detect its signed volume
/// Jacobian before handing a singular skin to adaptive face meshing. This
/// checks local regularity, not distant intersections between separate runs.
/// A consistently negative Jacobian remains regular for an off-path anchor;
/// return that orientation so the complete shell can be turned accordingly.
/// With `folds` a local fold is accepted (as the reference modeller keeps a
/// twisted or scaled sweep whose sections overlap round a tight bend) and
/// the orientation most samples have is returned.
fn regular_transport(wires: &[Wire], patches: &[Patch], folds: bool) -> Option<bool> {
    let mut points = Vec::new();
    for curve in wires.iter().flat_map(|wire| &wire.curves) {
        let spline = NurbsCurve::new_strict(curve.degree, curve.points.clone(),
            curve.knots.clone(), curve.weights.clone())?;
        let mut cuts = vec![0.0, 1.0];
        cuts.extend(curve.knots.iter().copied().filter(|knot| *knot > 0.0 && *knot < 1.0));
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        let samples = (curve.degree * 4).max(8);
        for span in cuts.windows(2) {
            for sample in 0..=samples {
                points.push(spline.point_at(span[0] + (span[1] - span[0]) * sample as f64 / samples as f64));
            }
        }
    }
    let mut orientation = None;
    let mut balance = 0_i64;
    for patch in patches {
        for at in [0.0, 0.125, 0.25, 0.5, 0.75, 0.875, 1.0] {
            let frame = bezier(patch, at);
            let derivative = bezier_derivative(patch, at);
            let normal = frame.x.cross(frame.y);
            let normal_length = normal.length();
            if !normal_length.is_finite() || normal_length <= 0.0 { return None; }
            for point in &points {
                let velocity = derivative.point(*point);
                let jacobian = normal.dot(velocity);
                let tolerance = normal_length * velocity.length() * 1e-10;
                if !jacobian.is_finite() || !tolerance.is_finite() { return None; }
                if jacobian.abs() <= tolerance {
                    if folds { continue; }
                    return None;
                }
                let forward = jacobian > 0.0;
                balance += if forward { 1 } else { -1 };
                if orientation.is_some_and(|previous| previous != forward) && !folds { return None; }
                orientation = Some(forward);
            }
        }
    }
    if folds && balance != 0 { return Some(balance > 0); }
    orientation
}

fn frame_error(a: Frame, b: Frame, radius: f64) -> f64 {
    a.origin.distance(b.origin) + radius * (a.x.distance(b.x) + a.y.distance(b.y))
}

fn build_body(wires: &[Wire], patches: &[Patch], sheet: bool, closed_path: bool, outward: bool) -> Option<Body> {
    build_body_in_runs(wires, patches, &vec![1; patches.len()], sheet, closed_path, outward, false, None)
}

/// A profile point placed on a section frame, lifted along the section
/// normal (scaled with the section) by its height.
fn lifted_point(frame: &Frame, point: [f64; 2], height: f64) -> Vec3 {
    let point3 = frame.point(point);
    if height == 0.0 { return point3; }
    let scale = frame.y.length();
    let normal = frame.x.cross(frame.y) * (1.0 / scale.max(1e-300));
    point3 + normal * height
}

/// A whole round wire as one closed rational curve (C0 where its arcs
/// meet), so the swept side is a single face, as the reference builds a
/// twisted or scaled round sweep.
fn merged_round(wire: &Wire) -> Option<Wire> {
    profile_circle(&wire.source)?;
    if !wire.closed || wire.curves.len() < 2 { return None; }
    let degree = wire.curves[0].degree;
    let count = wire.curves.len() as f64;
    let mut merged = RationalCurve2 { degree, knots: Vec::new(), points: Vec::new(), weights: Vec::new() };
    for (index, curve) in wire.curves.iter().enumerate() {
        if curve.degree != degree { return None; }
        let (low, high) = (*curve.knots.first()?, *curve.knots.last()?);
        let map = |k: f64| (index as f64 + (k - low) / (high - low)) / count;
        if index == 0 {
            merged.points.extend_from_slice(&curve.points);
            merged.weights.extend_from_slice(&curve.weights);
            merged.knots.extend(curve.knots[..curve.knots.len() - 1].iter().map(|k| map(*k)));
        } else {
            let scale = merged.weights.last()? / curve.weights.first()?;
            merged.points.extend_from_slice(&curve.points[1..]);
            merged.weights.extend(curve.weights[1..].iter().map(|w| w * scale));
            merged.knots.extend(curve.knots[degree + 1..curve.knots.len() - 1].iter().map(|k| map(*k)));
        }
        if index + 1 == wire.curves.len() {
            merged.knots.push(1.0);
        }
    }
    Some(Wire { source: wire.source.clone(), curves: vec![merged], closed: true })
}

/// The patches of one run joined into a single rational cubic B-spline in
/// the path direction, so each profile curve gives one face per path run,
/// as the reference modeler builds a twisted or scaled sweep. Knots are
/// triple (C0 in the knot vector) and spaced by the patches' spans, so the
/// Hermite patches' shared end derivatives make the run smooth. Weights are
/// rescaled so neighbouring patches agree at the shared frame.
fn joined_run(patches: &[Patch]) -> (Vec<Frame>, Vec<f64>, Vec<f64>) {
    let total = patches.iter().map(|patch| patch.span).sum::<f64>();
    let mut frames = vec![patches[0][0]];
    let mut weights = vec![patches[0].weights[0]];
    let mut knots = vec![0.0; 4];
    let mut at = 0.0;
    for (index, patch) in patches.iter().enumerate() {
        let scale = weights.last().copied().unwrap_or(1.0) / patch.weights[0];
        for k in 1..4 {
            frames.push(patch[k]);
            weights.push(patch.weights[k] * scale);
        }
        at += patch.span;
        let last = index + 1 == patches.len();
        let end = if last || !(total > 0.0) { if last { 1.0 } else { (index + 1) as f64 / patches.len() as f64 } } else { at / total };
        knots.extend(std::iter::repeat_n(end, if last { 4 } else { 3 }));
    }
    (frames, weights, knots)
}

#[allow(clippy::too_many_arguments)]
fn build_body_in_runs(wires: &[Wire], patches: &[Patch], runs: &[usize], sheet: bool, closed_path: bool, outward: bool, round: bool, lift: Option<&[f64]>) -> Option<Body> {
    // Heights of a lifted (spatial) single wire of straight pieces, by vertex.
    let height = |index: usize| lift.map_or(0.0, |heights| heights.get(index).copied().unwrap_or(0.0));
    let merged = wires.iter().map(|wire| round.then(|| merged_round(wire)).flatten()).collect::<Vec<_>>();
    let wires = &wires.iter().zip(merged).map(|(wire, merged)| merged.unwrap_or_else(|| Wire {
        source: wire.source.clone(), curves: wire.curves.clone(), closed: wire.closed })).collect::<Vec<_>>();
    if patches.is_empty() || runs.iter().sum::<usize>() != patches.len() || runs.contains(&0) { return None; }
    let mut groups = Vec::new();
    let mut at = 0;
    for count in runs {
        groups.push(&patches[at..at + count]);
        at += count;
    }
    let mut body = Body::new();
    let lump = body.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
    let shell = body.shells.insert(Shell { faces: Vec::new(), owner: lump, provenance: Provenance::Synthesized });
    let mut cap_rims = Vec::new();
    for wire in wires {
        let mut coords = wire.curves.iter().map(|curve| rational_point(curve, 0.0)).collect::<Option<Vec<_>>>()?;
        if !wire.closed { coords.push(rational_point(wire.curves.last()?, 1.0)?); }
        let mut vertices: Vec<Vec<VertexKey>> = Vec::new();
        let mut rims: Vec<Vec<EdgeKey>> = Vec::new();
        for station in 0..=groups.len() {
            if closed_path && station == groups.len() {
                vertices.push(vertices[0].clone());
                rims.push(rims[0].clone());
                continue;
            }
            let frame = if station == 0 { patches[0][0] } else { groups[station - 1].last()?[3] };
            let ring = coords.iter().enumerate().map(|(index, p)| body.vertices.insert(Vertex {
                point: lifted_point(&frame, *p, height(index)).to_array(), provenance: Provenance::Synthesized,
            })).collect::<Vec<_>>();
            let edges = wire.curves.iter().enumerate().map(|(index, curve)| {
                let next = (index + 1) % coords.len();
                let lifted = if lift.is_some() {
                    RationalCurve3 { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], weights: vec![1.0, 1.0],
                        points: vec![lifted_point(&frame, coords[index], height(index)).to_array(),
                            lifted_point(&frame, coords[next], height(next)).to_array()] }
                } else {
                    curve.lifted(&frame.plane())
                };
                add_curve_edge(&mut body, &lifted, ring[index], ring[next])
            }).collect::<Option<Vec<_>>>()?;
            vertices.push(ring);
            rims.push(edges);
        }
        for (band, group) in groups.iter().enumerate() {
            let (frames, along, knots) = joined_run(group);
            // Knots run over [0, 1], so knot and normalized parameters agree.
            let span = 1.0;
            let rails = coords.iter().enumerate().map(|(index, point)| {
                let curve = RationalCurve3 { degree: 3, knots: knots.clone(),
                    points: frames.iter().map(|frame| lifted_point(frame, *point, height(index)).to_array()).collect(),
                    weights: along.clone() };
                add_curve_edge(&mut body, &curve, vertices[band][index], vertices[band + 1][index])
            }).collect::<Option<Vec<_>>>()?;
            for (index, curve) in wire.curves.iter().enumerate() {
                let next = (index + 1) % coords.len();
                let ends = [height(index), height(next)];
                let points = curve.points.iter().enumerate().map(|(k, point)| {
                    let h = if lift.is_some() { ends[k.min(1)] } else { 0.0 };
                    frames.iter().map(|frame| lifted_point(frame, *point, h).to_array()).collect()
                }).collect();
                let weights = curve.weights.iter()
                    .map(|weight| along.iter().map(|along| weight * along).collect())
                    .collect();
                let surface = NurbsSurface3::new_strict(curve.degree, 3, points,
                    curve.knots.clone(), knots.clone(), weights)?;
                let surface = body.surfaces.insert(Surface::Nurbs(surface));
                let mut circuit = vec![(rims[band][index], true), (rails[next], true),
                    (rims[band + 1][index], false), (rails[index], false)];
                let mut pcurves = vec![([0.0, 0.0], [1.0, 0.0]), ([1.0, 0.0], [1.0, span]),
                    ([1.0, span], [0.0, span]), ([0.0, span], [0.0, 0.0])];
                if !outward {
                    circuit = circuit.into_iter().rev().map(|(edge, forward)| (edge, !forward)).collect();
                    pcurves = pcurves.into_iter().rev().map(|(a, b)| (b, a)).collect();
                }
                let face = add_face(&mut body, shell, surface, outward);
                add_loop(&mut body, face, &circuit, Some(&pcurves))?;
            }
        }
        cap_rims.push((rims[0].clone(), rims.last()?.clone()));
    }
    if !sheet && !closed_path {
        for end in [false, true] {
            let frame = if end { patches.last()?[3] } else { patches[0][0] };
            let surface = body.surfaces.insert(Surface::Plane(frame.plane()));
            let forward = if end { outward } else { !outward };
            let face = add_face(&mut body, shell, surface, forward);
            for (start_edges, end_edges) in &cap_rims {
                let edges = if end { end_edges } else { start_edges };
                let mut circuit = edges.iter().map(|edge| (*edge, true)).collect::<Vec<_>>();
                if !forward { circuit = circuit.into_iter().rev().map(|(edge, _)| (edge, false)).collect(); }
                // Like extrusion caps, the planar caps carry no pcurves: the
                // reference modeler rejects these solids with them.
                add_loop(&mut body, face, &circuit, None)?;
            }
        }
    }
    body.lumps.get_mut(lump)?.shells = vec![shell];
    body.roots = vec![lump];
    body.validate().is_empty().then_some(body)
}

fn rational_point(curve: &RationalCurve2, parameter: f64) -> Option<[f64; 2]> {
    // Unclamped/periodic splines do not end at their outer control points.
    // Evaluate the actual boundary so rims and swept rails share endpoints.
    Some(NurbsCurve::new_strict(curve.degree, curve.points.clone(), curve.knots.clone(),
        curve.weights.clone())?.point_at(parameter))
}


fn add_curve_edge(body: &mut Body, source: &RationalCurve3, start: VertexKey, end: VertexKey) -> Option<EdgeKey> {
    let curve = body.curves.insert(Curve3::Nurbs(source.curve()?));
    let (start_parameter, end_parameter) = (*source.knots.first()?, *source.knots.last()?);
    Some(body.edges.insert(Edge { curve, start_parameter, end_parameter, start, end,
        coedges: Vec::new(), provenance: Provenance::Synthesized }))
}

fn add_face(body: &mut Body, shell: ShellKey, surface: super::SurfaceKey, forward: bool) -> FaceKey {
    let face = body.faces.insert(Face { surface, forward, loops: Vec::new(), owner: shell, provenance: Provenance::Synthesized });
    body.shells.get_mut(shell).unwrap().faces.push(face);
    face
}

fn add_loop(body: &mut Body, face: FaceKey, circuit: &[(EdgeKey, bool)],
    pcurves: Option<&[([f64; 2], [f64; 2])]>,
) -> Option<super::LoopKey> {
    let ring = body.loops.insert(Loop { coedges: Vec::new(), owner: face, provenance: Provenance::Synthesized });
    for (index, (edge, forward)) in circuit.iter().enumerate() {
        let pcurve = pcurves.map(|curves| Curve::Line(Line { start: curves[index].0, end: curves[index].1 }));
        let coedge = body.coedges.insert(Coedge { edge: *edge, forward: *forward, pcurve, owner: ring,
            provenance: Provenance::Synthesized });
        body.edges.get_mut(*edge)?.coedges.push(coedge);
        body.loops.get_mut(ring)?.coedges.push(coedge);
    }
    body.faces.get_mut(face)?.loops.push(ring);
    Some(ring)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_geom::geom2d::Circle;

    fn round_profile() -> Vec<Vec<Curve>> {
        vec![vec![Curve::Circle(Circle { centre: [0.0, 0.0], radius: 1.0 })]]
    }

    const BENT: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 10.0]];

    #[test]
    fn a_round_profile_round_a_corner_has_exact_faces() {
        let path = SweepPath::Polyline3d { points: &BENT, closed: false };
        let body = sweep_path(Plane::XY, &round_profile(), path, SweepOptions::default()).unwrap();
        assert!(body.validate().is_empty());
        let surfaces = || body.faces.iter().filter_map(|(_, face)| body.surfaces.get(face.surface));
        assert!(surfaces().any(|surface| matches!(surface, Surface::Cylinder(_))));
        assert!(!surfaces().any(|surface| matches!(surface, Surface::Nurbs(_))));
    }

    #[test]
    fn a_twist_round_a_corner_builds_but_a_record_refuses_it() {
        let path = || SweepPath::Polyline3d { points: &BENT, closed: false };
        let options = SweepOptions { twist: 1.0, ..SweepOptions::default() };
        assert!(sweep_path(Plane::XY, &round_profile(), path(), options).is_some());
        assert_eq!(sweep_corner_refusal(path(), options), Some(SweepRefusal::Twist));
    }
}

/// Whether an axis through `pivot` leaves the profile's plane: such a turn
/// sweeps surfaces no analytic kind describes.
pub(crate) fn axis_off_plane(plane: &Plane, pivot: [f64; 3], axis: [f64; 3]) -> bool {
    let (Some(normal), Some(direction)) = (plane.normal(), Vec3::from(axis).normalize()) else { return false };
    direction.dot(Vec3::from(normal)).abs() > 1e-9 || plane.distance_to(pivot).is_some_and(|d| d.abs() > 1e-9)
}

/// A planar profile turned about an axis outside its plane. Each profile
/// curve sweeps one exact rational surface of revolution (the profile
/// curve across, quarter-turn rational arcs round), as the reference
/// builds its rotational spline surfaces; a whole turn closes on itself and
/// a solid's partial turn is capped by the profile at both ends.
pub fn revolve_off_plane(plane: Plane, profile: &[Curve], pivot: [f64; 3], axis: [f64; 3], angle: f64, sheet: bool) -> Option<Body> {
    let mut direction = Vec3::from(axis).normalize()?;
    let mut angle = angle;
    if angle < 0.0 {
        direction = -direction;
        angle = -angle;
    }
    if !angle.is_finite() || angle <= 1e-12 || angle > TAU + 1e-9 {
        return None;
    }
    let closed = (TAU - angle).abs() <= 1e-9;
    let wires = prepare_wires(&[profile.to_vec()])?;
    if !sheet && !wires[0].closed {
        return None;
    }
    let first = Frame { origin: Vec3::from(plane.origin), x: Vec3::from(plane.x_axis), y: Vec3::from(plane.y_axis) };
    let mut patches = Vec::new();
    turning_patches(first, Vec3::from(pivot), direction, angle.min(TAU), &mut patches);
    if closed {
        let start = patches.first()?[0];
        patches.last_mut()?[3] = start;
    }
    let outward = if sheet { true } else { regular_transport(&wires, &patches, false)? };
    build_body_in_runs(&wires, &patches, &[patches.len()], sheet, closed, outward, false, None)
}
