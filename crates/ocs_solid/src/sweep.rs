//! A profile swept along a path as the reference modeller's SWEEP places it: moved so its base
//! point stands at the path's start, turned square to the path there (when aligned) and by its
//! rotation, then twisted and scaled along the path. Where the profile goes is decided here;
//! the swept body is Solidcore's: its sweep along a smooth path, its controlled sweep for a
//! twist or a scale along lines and arcs, a loft through placed sections for one along a
//! spline, and a mitre at each corner of a path with corners (each piece swept, cut at the
//! corner's bisecting plane and the pieces united).

use std::f64::consts::{PI, TAU};
#[cfg(test)]
use std::f64::consts::FRAC_PI_2;

use ocs_geom::geom2d::{Arc, Curve, NurbsCurve};
use ocs_geom::rational::RationalCurve2;
use ocs_geom::space::{NurbsCurve3, Plane, Vec3};
use solidcore_ops::sweep;

use crate::model::{self, BooleanOp, Brep, Error, Placement};

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
    /// Axes of the profile's own plane where it was drawn, when the profile supplied has
    /// already been placed on the path; `None` uses the supplied profile plane.
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

const GAUSS: [(f64, f64); 5] = [
    (-0.906_179_845_938_664, 0.236_926_885_056_189),
    (-0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.0, 0.568_888_888_888_889),
    (0.538_469_310_105_683, 0.478_628_670_499_366),
    (0.906_179_845_938_664, 0.236_926_885_056_189),
];

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
            Self::Planar(plane, curve, forward) => plane.point_at(curve.point_at(if *forward { t } else { 1.0 - t })),
            Self::Spline(curve, a, b) => curve.point_at_knot(a + (b - a) * t),
        })
    }

    fn tangent(&self, t: f64) -> Option<Vec3> {
        let direction = match self {
            Self::Line(a, b) => *b - *a,
            Self::Planar(plane, curve, forward) => {
                Vec3::from(plane.vector_at(curve.tangent_at(if *forward { t } else { 1.0 - t }))) * if *forward { 1.0 } else { -1.0 }
            }
            Self::Spline(..) => self.spline_derivative(t),
        };
        direction.is_finite().then_some(())?;
        if direction.length() > 1e-12 {
            return direction.normalize();
        }
        (self.point((t + 1e-6).min(1.0)) - self.point((t - 1e-6).max(0.0))).normalize()
    }

    fn speed(&self, t: f64) -> f64 {
        match self {
            Self::Line(a, b) => a.distance(*b),
            Self::Planar(plane, curve, forward) => Vec3::from(plane.vector_at(curve.tangent_at(if *forward { t } else { 1.0 - t }))).length(),
            Self::Spline(..) => self.spline_derivative(t).length(),
        }
    }

    fn spline_derivative(&self, t: f64) -> Vec3 {
        let Self::Spline(curve, a, b) = self else { return Vec3::ZERO };
        let inset = (b - a) * 1e-10;
        Vec3::from(curve.derivative_at_knot((a + (b - a) * t).clamp(a + inset, b - inset))) * (b - a)
    }

    fn length_to(&self, end: f64) -> f64 {
        if let Self::Line(a, b) = self {
            return a.distance(*b) * end;
        }
        if let Self::Planar(plane, curve, _) = self {
            if let Curve::Line(line) = curve {
                return Vec3::from(plane.vector_at(line.direction())).length() * end;
            }
            if matches!(curve, Curve::Arc(_)) && plane.is_orthonormal() {
                return curve.length() * end;
            }
        }
        let panels = 16;
        (0..panels)
            .map(|panel| {
                GAUSS
                    .into_iter()
                    .map(|(node, weight)| {
                        let t = end * (panel as f64 + 0.5 + node * 0.5) / panels as f64;
                        self.speed(t) * weight * end / (2 * panels) as f64
                    })
                    .sum::<f64>()
            })
            .sum()
    }

    fn length(&self) -> f64 {
        self.length_to(1.0)
    }
}

fn near2(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8 + a.into_iter().chain(b).map(f64::abs).fold(1.0_f64, f64::max) * f64::EPSILON * 64.0
}

/// Polylines into their spans and circles into full arcs, keeping each span's direction.
fn expanded(curves: &[Curve]) -> Option<Vec<Curve>> {
    let mut result = Vec::new();
    for curve in curves {
        match curve {
            Curve::Polyline(polyline) => {
                for (index, segment) in curve.segments().into_iter().enumerate() {
                    if near2(polyline.vertices[index].position, segment.point_at(0.0)) {
                        result.push(segment);
                    } else {
                        // A clockwise bulge is a reversed counter-clockwise arc.
                        let rational = RationalCurve2::from_curve(&segment)?.reversed();
                        result.push(Curve::Nurbs(NurbsCurve::new_strict(rational.degree, rational.points, rational.knots, rational.weights)?));
                    }
                }
            }
            Curve::Circle(circle) => result.push(Curve::Arc(Arc {
                centre: circle.centre,
                radius: circle.radius,
                start_angle: 0.0,
                end_angle: TAU,
            })),
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
            if !forward && !near2(point, piece.point_at(1.0)) {
                return None;
            }
            senses.push(forward);
            point = piece.point_at(if forward { 1.0 } else { 0.0 });
        }
        Some(senses)
    })
}

fn chain_closed(pieces: &[Curve], senses: &[bool]) -> bool {
    near2(
        pieces[0].point_at(if senses[0] { 0.0 } else { 1.0 }),
        pieces.last().unwrap().point_at(if *senses.last().unwrap() { 1.0 } else { 0.0 }),
    )
}

fn path_pieces(path: SweepPath<'_>) -> Option<Vec<Piece>> {
    let pieces = match path {
        SweepPath::Planar { plane, curves } => {
            plane.normal()?;
            let curves = expanded(curves)?;
            let senses = chain_senses(&curves)?;
            curves
                .into_iter()
                .zip(senses)
                .map(|(curve, forward)| match curve {
                    Curve::Line(line) => Piece::Line(
                        Vec3::from(plane.point_at(if forward { line.start } else { line.end })),
                        Vec3::from(plane.point_at(if forward { line.end } else { line.start })),
                    ),
                    _ => Piece::Planar(plane, curve, forward),
                })
                .collect::<Vec<_>>()
        }
        SweepPath::Polyline3d { points, closed } => {
            if points.len() < 2 || !points.iter().flatten().all(|v| v.is_finite()) {
                return None;
            }
            let mut pieces = points.windows(2).map(|pair| Piece::Line(Vec3::from(pair[0]), Vec3::from(pair[1]))).collect::<Vec<_>>();
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

fn rotate(value: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let (sin, cos) = angle.sin_cos();
    value * cos + axis.cross(value) * sin + axis * axis.dot(value) * (1.0 - cos)
}

/// `value` turned by the rotation that takes the unit `from` to the unit `to`.
fn transport(value: Vec3, from: Vec3, to: Vec3) -> Option<Vec3> {
    let cross = from.cross(to);
    let sin = cross.length();
    let cos = from.dot(to).clamp(-1.0, 1.0);
    if sin <= 1e-12 {
        if cos >= 0.0 {
            return Some(value);
        }
        let axis = if from.x.abs() < 0.8 { from.cross(Vec3::X) } else { from.cross(Vec3::Y) }.normalize()?;
        return Some(rotate(value, axis, PI));
    }
    Some(rotate(value, cross / sin, sin.atan2(cos)))
}

/// Curve-length weighted common anchor for several independent profiles.
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
    if !length.is_finite() || length <= 1e-14 || !moment.is_finite() {
        return None;
    }
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

/// Rigid placement of the original profile onto the first sweep section (no twist, scale one).
pub fn sweep_profile_placement(plane: Plane, wires: &[Vec<Curve>], path: SweepPath<'_>, options: SweepOptions) -> Option<Placement> {
    let pieces = path_pieces(path)?;
    let first = pieces.first()?;
    let base = Vec3::from(options.base_point.or_else(|| sweep_profile_base(plane, wires))?);
    initial_placement(plane, base, first.point(0.0), first.tangent(0.0)?, options)
}

fn initial_placement(plane: Plane, base: Vec3, start: Vec3, tangent: Vec3, options: SweepOptions) -> Option<Placement> {
    if !base.is_finite() || !start.is_finite() || !options.rotation.is_finite() {
        return None;
    }
    let normal = Vec3::from(plane.normal()?);
    let source_x = Vec3::from(plane.x_axis).normalize()?;
    let source_y = normal.cross(source_x).normalize()?;
    let projected_x = source_x - tangent * source_x.dot(tangent);
    let (target_x, target_y, target_normal) = if projected_x.length() > 1e-10 {
        let x = projected_x.normalize()?;
        (x, tangent.cross(x).normalize()?, tangent)
    } else {
        // A path parallel to the profile's first axis keeps the profile's cyclic frame;
        // reversing that path must not turn the selected profile over.
        (source_y, normal, source_x)
    };
    let axis = if options.align { target_normal } else { normal };
    let map = |vector: Vec3| -> Option<Vec3> {
        let aligned = if options.align {
            target_x * vector.dot(source_x) + target_y * vector.dot(source_y) + target_normal * vector.dot(normal)
        } else {
            vector
        };
        Some(rotate(aligned, axis, options.rotation))
    };
    Some(Placement {
        x_axis: map(Vec3::X)?.to_array(),
        y_axis: map(Vec3::Y)?.to_array(),
        z_axis: map(Vec3::Z)?.to_array(),
        origin: (start - map(base)?).to_array(),
    })
}

/// Default source-profile anchor. Closed conics use their centre and open conics their middle
/// point; other open chains use their middle point by length, and other closed chains the mean
/// of twenty equally spaced boundary samples including the directed endpoints. Multiple
/// boundary loops contribute in proportion to their curve lengths.
pub fn sweep_profile_base(plane: Plane, wires: &[Vec<Curve>]) -> Option<[f64; 3]> {
    plane.normal()?;
    let origin = Vec3::from(plane.point_at(wires.first()?.first()?.point_at(0.0)));
    let mut total = 0.0;
    let mut moment = Vec3::ZERO;
    for wire in wires {
        let pieces = expanded(wire)?;
        let senses = chain_senses(&pieces)?;
        let closed = chain_closed(&pieces, &senses);
        let path = pieces.iter().zip(senses).map(|(curve, forward)| Piece::Planar(plane, curve.clone(), forward)).collect::<Vec<_>>();
        let lengths = path.iter().map(Piece::length).collect::<Vec<_>>();
        let length = lengths.iter().sum::<f64>();
        if !length.is_finite() || length <= 1e-14 {
            return None;
        }
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
                    if path[index].length_to(middle) < distance {
                        low = middle;
                    } else {
                        high = middle;
                    }
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
    if !total.is_finite() || total <= 1e-14 || !moment.is_finite() {
        return None;
    }
    Some((origin + moment / total).to_array())
}

/// The base point of one profile swept along a path that starts at `start` with direction
/// `tangent`: a path leaving the profile's plane from inside its region or its boundary sweeps
/// the profile where it stands, so the start is the base; any other start, or a path along
/// the profile's plane, uses the profile's own anchor.
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
    let inside = wires.iter().filter(|wire| ocs_geom::geom2d::containment::contains(wire, uv, tolerance)).count() % 2 == 1;
    let on_boundary = wires.iter().flatten().any(|curve| ocs_geom::geom2d::containment::distance_to(curve, uv) <= tolerance.linear());
    Some(if inside || on_boundary { start } else { anchor })
}

fn conic_anchor(pieces: &[Curve]) -> Option<[f64; 2]> {
    let center = match pieces.first()? {
        Curve::Arc(first)
            if pieces.iter().all(|piece| {
                matches!(piece, Curve::Arc(arc)
                    if near2(first.centre, arc.centre) && (first.radius - arc.radius).abs() <= first.radius.abs().max(1.0) * 1e-10)
            }) =>
        {
            Some(first.centre)
        }
        Curve::Ellipse(first) if pieces.iter().all(|piece| matches!(piece, Curve::Ellipse(arc) if first.ellipse == arc.ellipse)) => {
            Some(first.ellipse.centre)
        }
        _ => None,
    }?;
    let senses = chain_senses(pieces)?;
    if chain_closed(pieces, &senses) {
        return Some(center);
    }
    let spans = pieces
        .iter()
        .map(|piece| match piece {
            Curve::Arc(arc) => arc.sweep(),
            Curve::Ellipse(arc) => arc.sweep(),
            _ => 0.0,
        })
        .collect::<Vec<_>>();
    let mut middle = spans.iter().sum::<f64>() * 0.5;
    let mut index = 0;
    while index + 1 < spans.len() && middle > spans[index] {
        middle -= spans[index];
        index += 1;
    }
    let parameter = middle / spans[index];
    Some(pieces[index].point_at(if senses[index] { parameter } else { 1.0 - parameter }))
}

fn is_corner(before: Vec3, after: Vec3) -> bool {
    before.dot(after) < 1.0 - 1e-9
}

/// The corners of a path's joints: at each, the index of the piece after it.
fn corners(pieces: &[Piece], closed: bool) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for (index, pair) in pieces.windows(2).enumerate() {
        if is_corner(pair[0].tangent(1.0)?, pair[1].tangent(0.0)?) {
            out.push(index + 1);
        }
    }
    if closed && is_corner(pieces.last()?.tangent(1.0)?, pieces[0].tangent(0.0)?) {
        out.push(0);
    }
    Some(out)
}

fn path_closed(pieces: &[Piece]) -> bool {
    let start = pieces[0].point(0.0);
    let end = pieces[pieces.len() - 1].point(1.0);
    let extent = pieces.iter().map(Piece::length).sum::<f64>();
    start.distance(end) <= extent.max(1.0) * 1e-9
}

/// Recorded sweeps refuse to twist or scale along a path with a corner (a mitred joint cannot
/// change the section on both sides), and to bank along a planar path with a corner, as the
/// reference modeler does.
pub fn sweep_corner_refusal(path: SweepPath<'_>, options: SweepOptions) -> Option<SweepRefusal> {
    let planar = matches!(path, SweepPath::Planar { .. });
    let pieces = path_pieces(path)?;
    let cornered = !corners(&pieces, path_closed(&pieces))?.is_empty();
    if !cornered {
        None
    } else if (options.scale - 1.0).abs() > 1e-12 {
        Some(SweepRefusal::Scale)
    } else if options.twist.abs() > 1e-12 {
        Some(SweepRefusal::Twist)
    } else if options.bank && planar {
        Some(SweepRefusal::Bank)
    } else {
        None
    }
}

/// Whether a sweep path has a corner (including a closed path's closing corner).
pub fn sweep_path_has_corner(path: SweepPath<'_>) -> Option<bool> {
    let pieces = path_pieces(path)?;
    Some(!corners(&pieces, path_closed(&pieces))?.is_empty())
}

/// The anchor of a spatial polyline profile, by 3D length: an open one's middle point, a closed
/// one's mean of twenty equally spaced samples along it (both ends included).
pub fn sweep_polyline_base(points: &[[f64; 3]], closed: bool) -> Option<[f64; 3]> {
    let mut chain = points.iter().map(|p| Vec3::from(*p)).collect::<Vec<_>>();
    if closed {
        chain.push(*chain.first()?);
    }
    let lengths = chain.windows(2).map(|pair| pair[0].distance(pair[1])).collect::<Vec<_>>();
    let total = lengths.iter().sum::<f64>();
    if chain.len() < 2 || !total.is_finite() || total <= 1e-14 {
        return None;
    }
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

/// A path's curves the other way round, each a NURBS over [0, 1].
pub fn reversed_path_curves(curves: &[Curve]) -> Option<Vec<Curve>> {
    let curves = expanded(curves)?;
    let senses = chain_senses(&curves)?;
    curves
        .iter()
        .zip(senses)
        .rev()
        .map(|(curve, forward)| {
            let mut rational = RationalCurve2::from_curve(curve)?;
            let (low, high) = (*rational.knots.first()?, *rational.knots.last()?);
            rational.knots = rational.knots.iter().map(|k| (k - low) / (high - low)).collect();
            let rational = if forward { rational.reversed() } else { rational };
            Some(Curve::Nurbs(NurbsCurve::new_strict(rational.degree, rational.points, rational.knots, rational.weights)?))
        })
        .collect()
}

// ── Curves for Solidcore ──────────────────────────────────────────────────────

/// A plane's axes when they are square and of one length: its unit x, unit normal and scale.
fn uniform(plane: &Plane) -> Option<(Vec3, Vec3, f64)> {
    let (x, y) = (Vec3::from(plane.x_axis), Vec3::from(plane.y_axis));
    let s = x.length();
    ((y.length() - s).abs() <= 1e-9 * s.max(1.0) && x.dot(y).abs() <= 1e-9 * s * s && s > 0.0)
        .then(|| Some((x.normalize()?, x.cross(y).normalize()?, s)))
        .flatten()
}

fn nurbs(degree: usize, points: &[[f64; 3]], weights: &[f64], knots: &[f64]) -> sweep::Curve {
    sweep::Curve::Nurbs {
        degree,
        points: points.iter().zip(weights).map(|(p, w)| [p[0], p[1], p[2], *w]).collect(),
        knots: knots.to_vec(),
    }
}

/// A sweep curve run the other way.
pub fn reversed(curve: &sweep::Curve) -> sweep::Curve {
    match curve {
        sweep::Curve::Line { start, end } => sweep::Curve::Line { start: *end, end: *start },
        sweep::Curve::Arc { center, axis, reference, radius, start, end } => sweep::Curve::Arc {
            center: *center,
            axis: axis.map(|a| -a),
            reference: *reference,
            radius: *radius,
            start: -end,
            end: -start,
        },
        sweep::Curve::Nurbs { degree, points, knots } => {
            let (low, high) = (knots[0], knots[knots.len() - 1]);
            sweep::Curve::Nurbs {
                degree: *degree,
                points: points.iter().rev().copied().collect(),
                knots: knots.iter().rev().map(|k| low + high - k).collect(),
            }
        }
    }
}

/// A 2D curve on `plane` as sweep curves, run forward or backward.
pub fn plane_curves(plane: &Plane, curve: &Curve, forward: bool) -> Option<Vec<sweep::Curve>> {
    let analytic = match (curve, uniform(plane)) {
        (Curve::Line(line), _) => Some(sweep::Curve::Line {
            start: plane.point_at(line.start),
            end: plane.point_at(line.end),
        }),
        (Curve::Arc(arc), Some((x, normal, s))) => Some(sweep::Curve::Arc {
            center: plane.point_at(arc.centre),
            axis: normal.to_array(),
            reference: x.to_array(),
            radius: arc.radius * s,
            start: arc.start_angle,
            end: arc.start_angle + arc.sweep(),
        }),
        (Curve::Circle(circle), Some((x, normal, s))) => Some(sweep::Curve::Arc {
            center: plane.point_at(circle.centre),
            axis: normal.to_array(),
            reference: x.to_array(),
            radius: circle.radius * s,
            start: 0.0,
            end: TAU,
        }),
        _ => None,
    };
    let curves = match analytic {
        Some(curve) => vec![curve],
        None => match curve {
            Curve::Polyline(_) => {
                let pieces = expanded(std::slice::from_ref(curve))?;
                let mut out = Vec::new();
                for piece in &pieces {
                    out.extend(plane_curves(plane, piece, true)?);
                }
                out
            }
            _ => {
                let lifted = RationalCurve2::from_curve(curve)?.lifted(plane);
                vec![nurbs(lifted.degree, &lifted.points, &lifted.weights, &lifted.knots)]
            }
        },
    };
    Some(if forward { curves } else { curves.iter().rev().map(reversed).collect() })
}

/// A 3D NURBS as a sweep curve.
pub fn nurbs3(curve: &NurbsCurve3) -> sweep::Curve {
    nurbs(curve.degree(), curve.control_points(), curve.weights(), curve.knots())
}

/// The wires of a profile on `plane`, each a chain of sweep curves run end to end, and whether
/// each is closed.
pub fn profile_loops(plane: &Plane, wires: &[Vec<Curve>]) -> Option<Vec<(Vec<sweep::Curve>, bool)>> {
    wires
        .iter()
        .map(|wire| {
            let pieces = expanded(wire)?;
            let senses = chain_senses(&pieces)?;
            let closed = chain_closed(&pieces, &senses);
            let mut out = Vec::new();
            for (piece, forward) in pieces.iter().zip(senses) {
                out.extend(plane_curves(plane, piece, forward)?);
            }
            Some((out, closed))
        })
        .collect()
}

/// A point and a direction moved by `map` (a frame change with a scale).
struct Map {
    from: [Vec3; 4],
    to: [Vec3; 4],
    scale: f64,
}

impl Map {
    fn vector(&self, v: Vec3) -> Vec3 {
        let [_, x, y, z] = self.from;
        let [_, tx, ty, tz] = self.to;
        tx * v.dot(x) + ty * v.dot(y) + tz * v.dot(z)
    }

    fn point(&self, p: [f64; 3]) -> [f64; 3] {
        (self.to[0] + self.vector(Vec3::from(p) - self.from[0]) * self.scale).to_array()
    }

    fn curve(&self, curve: &sweep::Curve) -> sweep::Curve {
        match curve {
            sweep::Curve::Line { start, end } => sweep::Curve::Line { start: self.point(*start), end: self.point(*end) },
            sweep::Curve::Arc { center, axis, reference, radius, start, end } => sweep::Curve::Arc {
                center: self.point(*center),
                axis: self.vector(Vec3::from(*axis)).to_array(),
                reference: self.vector(Vec3::from(*reference)).to_array(),
                radius: radius * self.scale,
                start: *start,
                end: *end,
            },
            sweep::Curve::Nurbs { degree, points, knots } => sweep::Curve::Nurbs {
                degree: *degree,
                points: points
                    .iter()
                    .map(|p| {
                        let q = self.point([p[0], p[1], p[2]]);
                        [q[0], q[1], q[2], p[3]]
                    })
                    .collect(),
                knots: knots.clone(),
            },
        }
    }
}

/// Every point a curve's definition names, to bound how far it reaches.
fn reach_points(curve: &sweep::Curve) -> Vec<Vec3> {
    match curve {
        sweep::Curve::Line { start, end } => vec![Vec3::from(*start), Vec3::from(*end)],
        sweep::Curve::Arc { center, radius, .. } => {
            let c = Vec3::from(*center);
            vec![c + Vec3::X * *radius, c - Vec3::X * *radius, c + Vec3::Y * *radius, c - Vec3::Y * *radius, c + Vec3::Z * *radius, c - Vec3::Z * *radius]
        }
        sweep::Curve::Nurbs { points, .. } => points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
    }
}

// ── Swept bodies ──────────────────────────────────────────────────────────────

/// An outer profile and its holes swept along a connected path. A single open wire gives a
/// sheet even when `surface` is false.
pub fn sweep_path(profile_plane: Plane, wires: &[Vec<Curve>], path: SweepPath<'_>, options: SweepOptions) -> Result<Brep, Error> {
    let pieces = path_pieces(path).ok_or(Error::Argument)?;
    let placement = sweep_profile_placement(profile_plane, wires, path, options).ok_or(Error::Argument)?;
    let placed_plane = Plane::from_axes(
        placement.point(profile_plane.origin),
        placement.vector(profile_plane.x_axis),
        placement.vector(profile_plane.y_axis),
    );
    let loops = profile_loops(&placed_plane, wires).ok_or(Error::Argument)?;
    let cap = !options.surface && loops.iter().all(|(_, closed)| *closed);
    let start = pieces[0].point(0.0);
    let tangent = pieces[0].tangent(0.0).ok_or(Error::Argument)?;
    let normal = Vec3::from(placed_plane.normal().ok_or(Error::Argument)?);
    let x = Vec3::from(placed_plane.x_axis).normalize().ok_or(Error::Argument)?;
    let frame = [start, x, normal.cross(x), normal];
    let mut body = swept(&loops[0].0, &pieces, frame, tangent, options, cap)?;
    for (hole, _) in &loops[1..] {
        let cut = swept(hole, &pieces, frame, tangent, options, cap)?;
        body = if cap {
            model::boolean(&body, &cut, BooleanOp::Subtract)?.ok_or(Error::Empty)?
        } else {
            body
        };
    }
    Ok(body)
}

/// One loop swept: its sections start in `frame` (the path's start, the profile's axes and its
/// normal) at the path's first tangent.
fn swept(profile: &[sweep::Curve], pieces: &[Piece], frame: [Vec3; 4], tangent: Vec3, options: SweepOptions, cap: bool) -> Result<Brep, Error> {
    let closed = path_closed(pieces);
    let cornered = !corners(pieces, closed).ok_or(Error::Argument)?.is_empty();
    let deformed = options.twist.abs() > 1e-12 || (options.scale - 1.0).abs() > 1e-12;
    if deformed {
        if let Some(controlled) = controlled_pieces(pieces).filter(|_| !cornered && !closed) {
            let laws = sweep::controlled::ControlledSweep {
                twist: options.twist,
                end_scale: options.scale,
                ..Default::default()
            };
            let (body, _) = sweep::controlled::sweep_controlled(profile, frame[3].to_array(), &controlled, &laws)?;
            return model::record(&body);
        }
        return stations(profile, pieces, frame, tangent, options, cap);
    }
    if cornered {
        return mitred(profile, pieces, frame, closed, cap);
    }
    let path = path_curves(pieces);
    let (body, _, _) = sweep::sweep_placed(profile, &path, sweep::SweepFrame::Follow, cap)?;
    model::record(&body)
}

/// A path as sweep curves.
fn path_curves(pieces: &[Piece]) -> Vec<sweep::Curve> {
    let mut out = Vec::new();
    let mut last_spline: Option<*const NurbsCurve3> = None;
    for piece in pieces {
        match piece {
            Piece::Line(a, b) => out.push(sweep::Curve::Line { start: a.to_array(), end: b.to_array() }),
            Piece::Planar(plane, curve, forward) => out.extend(plane_curves(plane, curve, *forward).unwrap_or_default()),
            // A spline's knot spans are one curve.
            Piece::Spline(curve, ..) => {
                if last_spline != Some(curve as *const _) {
                    out.push(nurbs3(curve));
                    last_spline = Some(curve as *const _);
                }
            }
        }
    }
    out
}

/// A path of lines and circular arcs as the controlled sweep's pieces.
fn controlled_pieces(pieces: &[Piece]) -> Option<Vec<sweep::controlled::Piece>> {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Line(a, b) => Some(sweep::controlled::Piece::Line { a: a.to_array(), b: b.to_array() }),
            Piece::Planar(plane, Curve::Arc(arc), forward) => {
                let (_, normal, _) = uniform(plane)?;
                let axis = if *forward { normal } else { -normal };
                Some(sweep::controlled::Piece::Arc {
                    centre: plane.point_at(arc.centre),
                    axis: axis.to_array(),
                    start: piece.point(0.0).to_array(),
                    angle: arc.sweep(),
                })
            }
            _ => None,
        })
        .collect()
}

/// A twisted or scaled sweep along a path the controlled sweep does not take (a spline): the
/// profile placed at stations along the path, each turned by the path's rotation-minimising
/// frame, twisted and scaled by how far along it is, and lofted through.
fn stations(profile: &[sweep::Curve], pieces: &[Piece], frame: [Vec3; 4], tangent: Vec3, options: SweepOptions, cap: bool) -> Result<Brep, Error> {
    let lengths: Vec<f64> = pieces.iter().map(Piece::length).collect();
    let total: f64 = lengths.iter().sum();
    const STATIONS: usize = 48;
    let at = |distance: f64| -> (Vec3, Vec3) {
        let mut d = distance;
        let mut index = 0;
        while index + 1 < pieces.len() && d > lengths[index] {
            d -= lengths[index];
            index += 1;
        }
        let t = (d / lengths[index]).clamp(0.0, 1.0);
        (pieces[index].point(t), pieces[index].tangent(t).unwrap_or(tangent))
    };
    let [origin, x0, y0, _] = frame;
    let mut sections = Vec::with_capacity(STATIONS + 1);
    let (mut x, mut y, mut previous) = (x0, y0, tangent);
    for i in 0..=STATIONS {
        let s = i as f64 / STATIONS as f64;
        let (point, t) = at(total * s);
        x = transport(x, previous, t).ok_or(Error::Argument)?;
        y = transport(y, previous, t).ok_or(Error::Argument)?;
        previous = t;
        let turn = options.twist * s;
        let map = Map {
            from: [origin, x0, y0, tangent],
            to: [point, rotate(x, t, turn), rotate(y, t, turn), t],
            scale: 1.0 + (options.scale - 1.0) * s,
        };
        sections.push(profile.iter().map(|c| map.curve(c)).collect::<Vec<_>>());
    }
    model::loft(&sections, 3, cap)
}

/// A sweep along a path with corners, mitred: each piece swept on its own from the section
/// transported to its start (lengthened past each corner by a straight run), cut by the plane
/// bisecting each corner, and the pieces united.
fn mitred(profile: &[sweep::Curve], pieces: &[Piece], frame: [Vec3; 4], closed: bool, cap: bool) -> Result<Brep, Error> {
    if pieces.iter().any(|piece| matches!(piece, Piece::Spline(..))) {
        let (body, _, _) = sweep::sweep_placed(profile, &path_curves(pieces), sweep::SweepFrame::Follow, cap)?;
        return model::record(&body);
    }
    let [origin, x0, y0, n0] = frame;
    let reach = profile
        .iter()
        .flat_map(reach_points)
        .map(|p| p.distance(origin))
        .fold(0.0, f64::max)
        .max(1e-6);
    let count = pieces.len();
    let corner_after = |i: usize| -> Option<(Vec3, Vec3)> {
        let next = if i + 1 < count { Some(i + 1) } else if closed { Some(0) } else { None }?;
        let (before, after) = (pieces[i].tangent(1.0)?, pieces[next].tangent(0.0)?);
        is_corner(before, after).then(|| (pieces[i].point(1.0), (before + after).normalize().unwrap_or(before)))
    };
    let corner_before = |i: usize| -> Option<(Vec3, Vec3)> {
        let previous = if i > 0 { i - 1 } else if closed { count - 1 } else { return None };
        corner_after(previous)
    };
    let mut result: Option<Brep> = None;
    let (mut x, mut y, mut n) = (x0, y0, n0);
    let mut tangent = pieces[0].tangent(0.0).ok_or(Error::Argument)?;
    for (i, piece) in pieces.iter().enumerate() {
        let start_tangent = piece.tangent(0.0).ok_or(Error::Argument)?;
        // The section turned onto this piece across the corner before it.
        x = transport(x, tangent, start_tangent).ok_or(Error::Argument)?;
        y = transport(y, tangent, start_tangent).ok_or(Error::Argument)?;
        n = transport(n, tangent, start_tangent).ok_or(Error::Argument)?;
        let start = piece.point(0.0);
        let end_tangent = piece.tangent(1.0).ok_or(Error::Argument)?;
        let end = piece.point(1.0);
        // Lengthened past each corner far enough that the bisecting cut runs through the
        // section whole.
        let lead = |corner: Option<(Vec3, Vec3)>, along: Vec3| -> f64 {
            match corner {
                Some((_, bisector)) => {
                    let cos = bisector.dot(along).abs().max(0.05);
                    reach * (1.0 / cos + 1.0)
                }
                None => 0.0,
            }
        };
        let (before, after) = (corner_before(i), corner_after(i));
        let ahead = lead(before, start_tangent);
        let behind = lead(after, end_tangent);
        let mut path = Vec::new();
        let first = start - start_tangent * ahead;
        if ahead > 0.0 {
            path.push(sweep::Curve::Line { start: first.to_array(), end: start.to_array() });
        }
        path.extend(path_curves(std::slice::from_ref(piece)));
        if behind > 0.0 {
            path.push(sweep::Curve::Line { start: end.to_array(), end: (end + end_tangent * behind).to_array() });
        }
        let map = Map {
            from: [origin, x0, y0, n0],
            to: [first, x, y, n],
            scale: 1.0,
        };
        let section: Vec<sweep::Curve> = profile.iter().map(|c| map.curve(c)).collect();
        let (body, _, _) = sweep::sweep_placed(&section, &path, sweep::SweepFrame::Follow, cap)?;
        let mut body = model::record(&body)?;
        let extent = (piece.length() + ahead + behind + reach) * 4.0;
        // Keep what lies past the corner before (in front of its bisector) and short of the
        // corner after (behind it).
        if let Some((point, bisector)) = before {
            if let Some((_, front)) = model::split_by_plane(&body, point.to_array(), bisector.to_array(), extent)? {
                body = front;
            }
        }
        if let Some((point, bisector)) = after {
            if let Some((back, _)) = model::split_by_plane(&body, point.to_array(), bisector.to_array(), extent)? {
                body = back;
            }
        }
        result = Some(match result {
            None => body,
            Some(so_far) => model::boolean(&so_far, &body, BooleanOp::Union)?.ok_or(Error::Empty)?,
        });
        // The section carried along this piece to its end.
        x = transport(x, start_tangent, end_tangent).ok_or(Error::Argument)?;
        y = transport(y, start_tangent, end_tangent).ok_or(Error::Argument)?;
        n = transport(n, start_tangent, end_tangent).ok_or(Error::Argument)?;
        tangent = end_tangent;
    }
    result.ok_or(Error::Empty)
}

/// A spatial (non-planar) polyline profile swept along a path, a sheet: placed as a planar
/// profile is, at its anchor.
pub fn sweep_spatial_polyline(points: &[[f64; 3]], closed: bool, path: SweepPath<'_>, options: SweepOptions) -> Result<Brep, Error> {
    if points.len() < 2 {
        return Err(Error::Argument);
    }
    let pieces = path_pieces(path).ok_or(Error::Argument)?;
    let base = Vec3::from(options.base_point.or_else(|| sweep_polyline_base(points, closed)).ok_or(Error::Argument)?);
    let start = pieces[0].point(0.0);
    let tangent = pieces[0].tangent(0.0).ok_or(Error::Argument)?;
    // The profile's plan, the world's XY through its anchor.
    let plan = Plane::from_axes(base.to_array(), [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let placement = initial_placement(plan, base, start, tangent, options).ok_or(Error::Argument)?;
    let mut corners_list: Vec<[f64; 3]> = points.iter().map(|p| placement.point(*p)).collect();
    if closed {
        corners_list.push(corners_list[0]);
    }
    let profile: Vec<sweep::Curve> = corners_list
        .windows(2)
        .filter(|pair| Vec3::from(pair[0]).distance(Vec3::from(pair[1])) > 1e-12)
        .map(|pair| sweep::Curve::Line { start: pair[0], end: pair[1] })
        .collect();
    let normal = Vec3::from(placement.vector([0.0, 0.0, 1.0])).normalize().ok_or(Error::Argument)?;
    let x = Vec3::from(placement.vector([1.0, 0.0, 0.0])).normalize().ok_or(Error::Argument)?;
    swept(&profile, &pieces, [start, x, normal.cross(x), normal], tangent, options, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(radius: f64) -> Vec<Vec<Curve>> {
        vec![vec![Curve::Circle(ocs_geom::geom2d::Circle { centre: [0.0, 0.0], radius })]]
    }

    #[test]
    fn a_circle_swept_round_a_corner_is_mitred() {
        let plane = Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let points = [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0], [10.0, 0.0, 10.0]];
        let body = sweep_path(plane, &circle(1.0), SweepPath::Polyline3d { points: &points, closed: false }, SweepOptions::default()).unwrap();
        let volume = model::volume(&body).unwrap();
        // A mitre keeps the section's area times the path's length.
        let expected = PI * 20.0;
        assert!((volume - expected).abs() < 1e-3 * expected, "{volume} vs {expected}");
    }

    #[test]
    fn a_square_twisted_along_a_line_keeps_its_volume() {
        let plane = Plane::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let square: Vec<Curve> = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
            .windows(2)
            .map(|w| Curve::Line(ocs_geom::geom2d::Line { start: w[0], end: w[1] }))
            .chain(std::iter::once(Curve::Line(ocs_geom::geom2d::Line { start: [-1.0, 1.0], end: [-1.0, -1.0] })))
            .collect();
        let points = [[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]];
        let options = SweepOptions { twist: FRAC_PI_2, ..SweepOptions::default() };
        let body = sweep_path(plane, &[square], SweepPath::Polyline3d { points: &points, closed: false }, options).unwrap();
        let volume = model::volume(&body).unwrap();
        assert!((volume - 40.0).abs() < 1e-2 * 40.0, "{volume}");
    }
}
