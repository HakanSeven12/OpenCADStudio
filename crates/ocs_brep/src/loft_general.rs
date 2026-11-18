//! Exact section-preserving lofts. Section compatibility is obtained by knot
//! insertion, Bézier subdivision and degree elevation, never by faceting curves.

use super::geometry::{Curve3, Surface};
use super::nurbs_builder::{RationalCurve2, RationalCurve3};
use super::topology::{Body, Coedge, Edge, EdgeKey, Face, Loop, Lump, Shell, ShellKey, Vertex, VertexKey};
use super::Provenance;
use ocs_geom::geom2d::{Curve, Line};
use ocs_geom::space::{NurbsSurface3, Plane, Vec3};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::fmt;

/// A planar section may contain an outer wire followed by holes. Points are
/// permitted only at the first or last section of a non-periodic loft.
#[derive(Debug, Clone)]
pub enum LoftSection {
    Profile { plane: Plane, wires: Vec<Vec<Curve>>, closed: bool },
    Point([f64; 3]),
}

#[derive(Debug, Clone, Copy)]
pub struct LoftOptions {
    pub surface: bool,
    /// 0 ruled, 1 smooth, 2 first normal, 3 last normal, 4 ends normal,
    /// 5 all normal, 6 endpoint draft angles.
    pub normals: i32,
    pub start_draft_angle: f64,
    pub end_draft_angle: f64,
    pub start_magnitude: f64,
    pub end_magnitude: f64,
    pub closed: bool,
    /// Smooth across the closure seam. Only applies to a closed loft.
    pub periodic: bool,
    pub align_direction: bool,
    pub start_continuity: i32,
    pub end_continuity: i32,
    pub start_bulge: f64,
    pub end_bulge: f64,
}

impl Default for LoftOptions {
    fn default() -> Self {
        Self {
            surface: false, normals: 1, start_draft_angle: FRAC_PI_2,
            end_draft_angle: FRAC_PI_2, start_magnitude: 0.0,
            end_magnitude: 0.0, closed: false, periodic: true, align_direction: true,
            start_continuity: 1, end_continuity: 1, start_bulge: 0.5, end_bulge: 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoftError(pub String);

impl fmt::Display for LoftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}

impl std::error::Error for LoftError {}

fn error(message: &str) -> LoftError { LoftError(message.to_owned()) }

type H = [f64; 4];

#[derive(Clone)]
struct Bezier { control: Vec<H> }

#[derive(Clone)]
struct Span { start: f64, end: f64, curve: Bezier }

#[derive(Clone)]
struct Wire { spans: Vec<Span>, closed: bool }

#[derive(Clone)]
struct Section {
    plane: Option<Plane>, wires: Vec<Wire>, point: Option<[f64; 3]>, centre: Vec3,
}

#[derive(Clone)]
struct Patch { control: Vec<Vec<H>> }

impl Bezier {
    fn degree(&self) -> usize { self.control.len() - 1 }
    fn point(&self, t: f64) -> [f64; 3] { project(de_casteljau(&self.control, t)) }
    fn reversed(&self) -> Self { Self { control: self.control.iter().rev().copied().collect() } }
    fn tangent(&self, t: f64) -> Vec3 {
        let h = de_casteljau(&self.control, t); let point = project(h);
        let derivative = self.control.windows(2).map(|p| std::array::from_fn(|i| (p[1][i] - p[0][i]) * self.degree() as f64)).collect::<Vec<H>>();
        let d = de_casteljau(&derivative, t);
        Vec3::new((d[0] - point[0] * d[3]) / h[3], (d[1] - point[1] * d[3]) / h[3], (d[2] - point[2] * d[3]) / h[3])
    }
    fn split(&self, t: f64) -> (Self, Self) {
        let mut work = self.control.clone();
        let mut left = vec![work[0]];
        let mut right = vec![*work.last().unwrap()];
        while work.len() > 1 {
            work = work.windows(2).map(|p| mix(p[0], p[1], t)).collect();
            left.push(work[0]); right.push(*work.last().unwrap());
        }
        right.reverse();
        (Self { control: left }, Self { control: right })
    }
    fn part(&self, a: f64, b: f64) -> Self {
        if a <= 1e-13 && b >= 1.0 - 1e-13 { return self.clone(); }
        let left = if b < 1.0 - 1e-13 { self.split(b).0 } else { self.clone() };
        if a > 1e-13 { left.split(a / b).1 } else { left }
    }
    fn elevated(&self, degree: usize) -> Self {
        let mut result = self.clone();
        while result.degree() < degree {
            let n = result.control.len();
            let mut control = vec![result.control[0]];
            for i in 1..n { control.push(mix(result.control[i], result.control[i - 1], i as f64 / n as f64)); }
            control.push(*result.control.last().unwrap());
            result.control = control;
        }
        result
    }
    fn unit_end_weights(&self) -> Self {
        // A projective parameter change leaves a rational Bézier's locus
        // unchanged. Unit endpoint weights give adjoining pieces the same
        // homogeneous rail data even when source curves use different scales.
        let first = self.control[0][3]; let last = self.control.last().unwrap()[3];
        let ratio = (first / last).powf(1.0 / self.degree() as f64);
        Self { control: self.control.iter().enumerate().map(|(i, h)| {
            let scale = ratio.powi(i as i32) / first;
            std::array::from_fn(|k| h[k] * scale)
        }).collect() }
    }
    fn eased_endpoint(&self, start: bool) -> Self {
        let parameter = if start { [0.0, 0.0, 2.0 / 3.0, 1.0] } else { [0.0, 1.0 / 3.0, 1.0, 1.0] };
        let mut polynomials = self.control.iter().map(|h| vec![*h]).collect::<Vec<_>>();
        while polynomials.len() > 1 {
            polynomials = polynomials.windows(2).map(|pair| {
                let n = pair[0].len() - 1;
                let mut result = vec![[0.0; 4]; n + 4];
                for i in 0..=n { for j in 0..=3 {
                    let factor = choose(n, i) * choose(3, j) / choose(n + 3, i + j);
                    for c in 0..4 { result[i + j][c] += ((1.0 - parameter[j]) * pair[0][i][c] + parameter[j] * pair[1][i][c]) * factor; }
                } }
                result
            }).collect();
        }
        Self { control: polynomials.remove(0) }
    }
    fn rational(&self) -> RationalCurve3 {
        RationalCurve3 {
            degree: self.degree(), knots: bezier_knots(self.degree()),
            points: self.control.iter().copied().map(project).collect(),
            weights: self.control.iter().map(|h| h[3]).collect(),
        }
    }
}

impl Wire {
    fn point(&self, t: f64) -> [f64; 3] {
        let span = self.spans.iter().find(|s| t <= s.end + 1e-12).unwrap_or_else(|| self.spans.last().unwrap());
        span.curve.point(((t - span.start) / (span.end - span.start)).clamp(0.0, 1.0))
    }
    fn part(&self, a: f64, b: f64) -> Result<Bezier, LoftError> {
        let span = self.spans.iter().find(|s| a >= s.start - 1e-10 && b <= s.end + 1e-10)
            .ok_or_else(|| error("A loft curve interval crosses an unsplit knot."))?;
        let width = span.end - span.start;
        Ok(span.curve.part(((a - span.start) / width).clamp(0.0, 1.0), ((b - span.start) / width).clamp(0.0, 1.0)))
    }
    fn reversed(&self) -> Self {
        Self { closed: self.closed, spans: self.spans.iter().rev().map(|s| Span {
            start: 1.0 - s.end, end: 1.0 - s.start, curve: s.curve.reversed(),
        }).collect() }
    }
    fn rotated(&self, at: f64) -> Result<Self, LoftError> {
        if !self.closed || at <= 1e-12 || at >= 1.0 - 1e-12 { return Ok(self.clone()); }
        let mut breaks = self.spans.iter().map(|s| s.start).collect::<Vec<_>>();
        breaks.extend([at, 1.0]); unique(&mut breaks);
        let mut spans = Vec::new();
        for pair in breaks.windows(2) {
            let start = (pair[0] - at).rem_euclid(1.0);
            spans.push(Span { start, end: start + pair[1] - pair[0], curve: self.part(pair[0], pair[1])? });
        }
        spans.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(Self { spans, closed: true })
    }
}

impl Patch {
    fn u_degree(&self) -> usize { self.control.len() - 1 }
    fn v_degree(&self) -> usize { self.control[0].len() - 1 }
    fn u_edge(&self, end: bool) -> Bezier {
        Bezier { control: self.control[if end { self.u_degree() } else { 0 }].clone() }
    }
    fn v_edge(&self, end: bool) -> Bezier {
        let v = if end { self.v_degree() } else { 0 };
        Bezier { control: self.control.iter().map(|row| row[v]).collect() }
    }
    fn v_part(&self, a: f64, b: f64) -> Self {
        Self { control: self.control.iter().map(|row| Bezier { control: row.clone() }.part(a, b).control).collect() }
    }
    fn elevated(&self, u: usize, v: usize) -> Self {
        let rows = self.control.iter().map(|row| Bezier { control: row.clone() }.elevated(v).control).collect::<Vec<_>>();
        let columns = (0..=v).map(|j| Bezier { control: rows.iter().map(|row| row[j]).collect() }.elevated(u).control).collect::<Vec<_>>();
        Self { control: (0..=u).map(|i| columns.iter().map(|column| column[i]).collect()).collect() }
    }
    fn surface(&self) -> Result<NurbsSurface3, LoftError> {
        NurbsSurface3::new_strict(
            self.u_degree(), self.v_degree(),
            self.control.iter().map(|r| r.iter().copied().map(project).collect()).collect(),
            bezier_knots(self.u_degree()), bezier_knots(self.v_degree()),
            self.control.iter().map(|r| r.iter().map(|h| h[3]).collect()).collect(),
        ).ok_or_else(|| error("Loft interpolation produced invalid rational weights."))
    }
}

fn mix(a: H, b: H, t: f64) -> H { std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t) }
fn project(h: H) -> [f64; 3] { [h[0] / h[3], h[1] / h[3], h[2] / h[3]] }
fn homogeneous(point: [f64; 3], weight: f64) -> H { [point[0] * weight, point[1] * weight, point[2] * weight, weight] }
fn de_casteljau(control: &[H], t: f64) -> H {
    let mut values = control.to_vec();
    while values.len() > 1 { values = values.windows(2).map(|p| mix(p[0], p[1], t)).collect(); }
    values[0]
}
fn bezier_knots(degree: usize) -> Vec<f64> {
    std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect()
}
fn unique(values: &mut Vec<f64>) {
    values.sort_by(f64::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() <= 1e-10);
}

/// Refines a homogeneous B-spline into its exact polynomial spans. Inserting
/// domain-end knots also handles unclamped and periodic source splines.
fn rational_spans(source: &RationalCurve3) -> Result<Vec<Span>, LoftError> {
    let p = source.degree;
    if p == 0 || p > 24 || source.points.len() <= p || source.knots.len() != source.points.len() + p + 1 {
        return Err(error("A loft section contains an invalid spline."));
    }
    let mut knots = source.knots.clone();
    let mut control = source.points.iter().zip(&source.weights).map(|(p, w)| homogeneous(*p, *w)).collect::<Vec<_>>();
    if control.len() != source.points.len() || control.iter().any(|h| !h.iter().all(|v| v.is_finite()) || h[3] <= 0.0) {
        return Err(error("Loft section weights must be finite and positive."));
    }
    let start = knots[p]; let end = knots[control.len()];
    if !start.is_finite() || !end.is_finite() || end - start <= 1e-12 || knots.windows(2).any(|k| k[1] < k[0]) {
        return Err(error("A loft section has an invalid knot domain."));
    }
    let mut breaks = knots.iter().copied().filter(|k| *k >= start && *k <= end).collect::<Vec<_>>();
    unique(&mut breaks);
    for at in &breaks {
        while knots.iter().filter(|k| (**k - *at).abs() <= 1e-12).count() < p {
            let n = control.len() - 1;
            // The active end of an unclamped curve is U[n + 1], not the
            // final control point. Insert on its right-hand span so the new
            // endpoint is the de Boor value rather than a copied pole.
            let k = if (*at - end).abs() <= 1e-12 { n + 1 } else {
                (p..=n).find(|i| knots[*i] <= *at && *at < knots[*i + 1]).ok_or_else(|| error("Spline knot refinement failed."))?
            };
            let s = knots.iter().filter(|knot| (**knot - *at).abs() <= 1e-12).count();
            let mut refined = vec![[0.0; 4]; control.len() + 1];
            refined[..=k - p].copy_from_slice(&control[..=k - p]);
            for i in k - s..=n { refined[i + 1] = control[i]; }
            for i in k - p + 1..=k - s {
                let width = knots[i + p] - knots[i];
                if width <= 0.0 { return Err(error("Spline knot refinement encountered a zero span.")); }
                refined[i] = mix(control[i - 1], control[i], (*at - knots[i]) / width);
            }
            knots.insert(k + 1, *at); control = refined;
        }
    }
    let mut result = Vec::new();
    for k in p..control.len() {
        if knots[k] < start - 1e-12 || knots[k + 1] > end + 1e-12 || knots[k + 1] - knots[k] <= 1e-12 { continue; }
        result.push(Span { start: (knots[k] - start) / (end - start), end: (knots[k + 1] - start) / (end - start),
            curve: Bezier { control: control[k - p..=k].to_vec() } });
    }
    if result.is_empty() { Err(error("A loft section contains no nonzero curve spans.")) } else { Ok(result) }
}

fn section_wire(plane: &Plane, pieces: &[Curve], closed: bool) -> Result<Wire, LoftError> {
    if pieces.is_empty() { return Err(error("A loft wire is empty.")); }
    let mut curves = pieces.iter().map(|piece| RationalCurve2::from_curve(piece).map(|c| c.lifted(plane))
        .ok_or_else(|| error("A loft section contains an unsupported curve."))).collect::<Result<Vec<_>, _>>()?;
    let tol = geometry_tolerance(curves.iter().flat_map(|c| c.points.iter().copied()), 1e-9);
    let ordered = [false, true].into_iter().find_map(|reverse_first| {
        let mut result = vec![if reverse_first { curves[0].reversed() } else { curves[0].clone() }];
        for c in &curves[1..] {
            let previous = result.last()?.curve()?.point_at_knot(1.0);
            let source = c.curve()?;
            if distance(previous, source.point_at_knot(0.0)) <= tol { result.push(c.clone()); }
            else if distance(previous, source.point_at_knot(1.0)) <= tol { result.push(c.reversed()); }
            else { return None; }
        }
        if closed && distance(result[0].curve()?.point_at_knot(0.0), result.last()?.curve()?.point_at_knot(1.0)) > tol { return None; }
        Some(result)
    }).ok_or_else(|| error("Loft section edges do not form a connected wire."))?;
    curves = ordered;
    let count = curves.len() as f64;
    let mut spans = Vec::new();
    for (index, curve) in curves.iter().enumerate() {
        for mut span in rational_spans(curve)? {
            span.start = (index as f64 + span.start) / count;
            span.end = (index as f64 + span.end) / count;
            spans.push(span);
        }
    }
    Ok(Wire { spans, closed })
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 { (Vec3::from(a) - Vec3::from(b)).length() }

fn geometry_tolerance(points: impl IntoIterator<Item = [f64; 3]>, relative: f64) -> f64 {
    let mut min = [f64::INFINITY; 3]; let mut max = [f64::NEG_INFINITY; 3]; let mut coordinate = 1.0_f64;
    for point in points { for i in 0..3 { min[i] = min[i].min(point[i]); max[i] = max[i].max(point[i]); coordinate = coordinate.max(point[i].abs()); } }
    let extent = (0..3).map(|i| max[i] - min[i]).fold(1.0_f64, f64::max);
    (extent * relative).max(coordinate * f64::EPSILON * 8.0)
}

fn rotate_between(vector: Vec3, from: Vec3, to: Vec3, fallback: Vec3) -> Vec3 {
    let cosine = from.dot(to).clamp(-1.0, 1.0);
    let cross = from.cross(to); let sine = cross.length();
    if sine <= 1e-12 {
        if cosine >= 0.0 { return vector; }
        let axis = (fallback - from * fallback.dot(from)).normalize().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        return axis * (2.0 * axis.dot(vector)) - vector;
    }
    let axis = cross / sine;
    vector * cosine + axis.cross(vector) * sine + axis * (axis.dot(vector) * (1.0 - cosine))
}

fn wire_centre(wire: &Wire) -> Vec3 {
    (0..32).map(|i| Vec3::from(wire.point((i as f64 + 0.5) / 32.0))).fold(Vec3::ZERO, |a, b| a + b) / 32.0
}

fn prepared(sections: &[LoftSection], options: LoftOptions) -> Result<Vec<Section>, LoftError> {
    if sections.len() < 2 { return Err(error("LOFT requires at least two cross sections.")); }
    if options.closed && sections.len() < 3 { return Err(error("A closed loft requires at least three cross sections.")); }
    if !(0..=6).contains(&options.normals) || ![options.start_draft_angle, options.end_draft_angle, options.start_magnitude, options.end_magnitude, options.start_bulge, options.end_bulge].iter().all(|v| v.is_finite())
        || options.start_magnitude < 0.0 || options.end_magnitude < 0.0 || options.start_bulge < 0.0 || options.end_bulge < 0.0
        || !(0..=1).contains(&options.start_continuity) || !(0..=1).contains(&options.end_continuity) {
        return Err(error("Loft normal or draft settings are invalid."));
    }
    let mut result = Vec::new();
    for (index, section) in sections.iter().enumerate() {
        result.push(match section {
            LoftSection::Point(point) => {
                if options.closed || (index != 0 && index + 1 != sections.len()) || !point.iter().all(|v| v.is_finite()) {
                    return Err(error("A loft point must be a finite first or last cross section of an open loft."));
                }
                Section { plane: None, wires: Vec::new(), point: Some(*point), centre: Vec3::from(*point) }
            }
            LoftSection::Profile { plane, wires, closed } => {
                if plane.normal().is_none() || wires.is_empty() || (!closed && wires.len() != 1) {
                    return Err(error("A loft section needs a valid plane and an outer wire; holes require closed profiles."));
                }
                let wires = wires.iter().map(|w| section_wire(plane, w, *closed)).collect::<Result<Vec<_>, _>>()?;
                Section { plane: Some(*plane), centre: wire_centre(&wires[0]), wires, point: None }
            }
        });
    }
    let profile = result.iter().find(|s| s.point.is_none()).ok_or_else(|| error("A loft needs at least one curve section."))?;
    let wire_count = profile.wires.len(); let closed = profile.wires[0].closed;
    if result.iter().filter(|s| s.point.is_none()).any(|s| s.wires.len() != wire_count || s.wires[0].closed != closed) {
        return Err(error("All loft profiles must have matching open/closed topology and hole counts."));
    }
    if wire_count > 1 && result.iter().any(|s| s.point.is_some()) { return Err(error("A point-ended loft cannot collapse multiple boundary loops into one point.")); }
    // Align whole wires before compatibility subdivision. Their exact geometry
    // remains unchanged; only traversal direction and the periodic seam move.
    if options.align_direction {
        // A supporting plane is unoriented: two identical profiles may store
        // opposite normals. Orient only the transport frames along the loft
        // progression, otherwise a harmless normal sign change reverses the
        // world-space perimeter and folds the skin between the sections.
        let transport_normals = result.iter().enumerate().map(|(index, section)| {
            let normal = Vec3::from(section.plane?.normal()?);
            let last = result.len() - 1;
            let before = if index > 0 { index - 1 } else if options.closed { last } else { 0 };
            let after = if index < last { index + 1 } else if options.closed { 0 } else { last };
            let mut travel = result[after].centre - result[before].centre;
            if travel.length() <= 1e-12 { travel = result[after].centre - section.centre; }
            Some(if normal.dot(travel) < 0.0 { -normal } else { normal })
        }).collect::<Vec<_>>();
        let mut previous: Option<(Vec<Wire>, Plane, Vec3, Vec3)> = None;
        for (index, section) in result.iter_mut().enumerate() {
            if section.point.is_some() { continue; }
            let current_normal = transport_normals[index].unwrap();
            if let Some((ref previous, old_plane, old_centre, old_normal)) = previous {
                for (wire, old) in section.wires.iter_mut().zip(previous) {
                    let mut best = wire.clone(); let mut best_cost = f64::INFINITY;
                    for candidate in [wire.clone(), wire.reversed()] {
                        let shifts = if closed { candidate.spans.iter().map(|s| s.start).collect() } else { vec![0.0] };
                        for shift in shifts {
                            let shifted = candidate.rotated(shift)?;
                            let cost = (0..24).map(|i| {
                                let at = i as f64 / 24.0;
                                let local = Vec3::from(shifted.point(at)) - section.centre;
                                let transported = rotate_between(local, current_normal, old_normal, Vec3::from(old_plane.x_axis)) + old_centre;
                                distance(old.point(at), transported.to_array()).powi(2)
                            }).sum::<f64>();
                            if cost < best_cost - 1e-12 { best_cost = cost; best = shifted; }
                        }
                    }
                    *wire = best;
                }
            }
            previous = Some((section.wires.clone(), section.plane.unwrap(), section.centre, current_normal));
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct Constraint {
    curve: Wire,
    parameters: Vec<f64>,
    anchors: Vec<f64>,
}

fn spatial_wire(curves: &[Curve3]) -> Result<Wire, LoftError> {
    if curves.is_empty() { return Err(error("A loft guide or path is empty.")); }
    let mut spans = Vec::new();
    for (i, curve) in curves.iter().enumerate() {
        let rational = match curve {
            Curve3::Line(line) => RationalCurve3 {
                degree: 1, knots: bezier_knots(1), points: vec![line.origin, (Vec3::from(line.origin) + Vec3::from(line.direction)).to_array()], weights: vec![1.0; 2],
            },
            Curve3::Nurbs(curve) => {
                let (a, b) = curve.domain();
                RationalCurve3 { degree: curve.degree(), knots: curve.knots().iter().map(|k| (k - a) / (b - a)).collect(),
                    points: curve.control_points().to_vec(), weights: curve.weights().to_vec() }
            }
            Curve3::PlanarSpline { plane, curve } => RationalCurve2::from_curve(&Curve::Nurbs(curve.clone()))
                .ok_or_else(|| error("Invalid loft guide spline."))?.lifted(plane),
            Curve3::Circle(circle) => {
                let mut c = RationalCurve2::unit_arc(TAU).ok_or_else(|| error("Invalid circular guide."))?;
                for point in &mut c.points { point[0] *= circle.radius; point[1] *= circle.radius; }
                c.lifted(&circle.plane)
            }
            Curve3::Ellipse(ellipse) => {
                let mut c = RationalCurve2::unit_arc(TAU).ok_or_else(|| error("Invalid elliptical guide."))?;
                for point in &mut c.points { point[0] *= ellipse.major_radius; point[1] *= ellipse.minor_radius; }
                c.lifted(&ellipse.plane)
            }
        };
        let mut part = rational_spans(&rational)?;
        if let Some(previous) = spans.last() {
            let previous: &Span = previous;
            let head = previous.curve.point(1.0);
            let tolerance = geometry_tolerance([head, part[0].curve.point(0.0), part.last().unwrap().curve.point(1.0)], 1e-9);
            if distance(head, part[0].curve.point(0.0)) > tolerance {
                if distance(head, part.last().unwrap().curve.point(1.0)) > tolerance {
                    return Err(error("Loft guide/path edges do not form a connected curve."));
                }
                part = Wire { spans: part, closed: false }.reversed().spans;
            }
        }
        for mut span in part {
            span.start = (i as f64 + span.start) / curves.len() as f64;
            span.end = (i as f64 + span.end) / curves.len() as f64;
            spans.push(span);
        }
    }
    let tolerance = geometry_tolerance(spans.iter().flat_map(|s| s.curve.control.iter().copied().map(project)), 1e-9);
    let closed = distance(spans[0].curve.point(0.0), spans.last().unwrap().curve.point(1.0)) <= tolerance;
    Ok(Wire { spans, closed })
}

fn closest_parameter(wire: &Wire, point: [f64; 3]) -> (f64, f64) {
    let mut best = (0.0, f64::INFINITY);
    for span in &wire.spans {
        for i in 0..24 {
            let mut a = i as f64 / 24.0; let mut b = (i + 1) as f64 / 24.0;
            for _ in 0..36 {
                let x = a + (b - a) / 3.0; let y = b - (b - a) / 3.0;
                if distance(span.curve.point(x), point) <= distance(span.curve.point(y), point) { b = y; } else { a = x; }
            }
            for t in [i as f64 / 24.0, (i + 1) as f64 / 24.0, (a + b) * 0.5] {
                let d = distance(span.curve.point(t), point);
                if d < best.1 { best = (span.start + (span.end - span.start) * t, d); }
            }
        }
    }
    best
}

fn plane_roots(curve: &Bezier, plane: Plane, tolerance: f64) -> Vec<f64> {
    let normal = Vec3::from(plane.normal().unwrap());
    let origin = Vec3::from(plane.origin);
    let mut result = Vec::new();
    let mut stack = vec![(curve.clone(), 0.0, 1.0, 0usize)];
    while let Some((part, a, b, depth)) = stack.pop() {
        let values = part.control.iter().map(|h| (Vec3::from(project(*h)) - origin).dot(normal)).collect::<Vec<_>>();
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if min > tolerance || max < -tolerance { continue; }
        if depth == 0 && min.abs().max(max.abs()) <= tolerance {
            // A path segment lying in a section plane does not define a unique
            // crossing. Its endpoints may still be the neighbouring crossings.
            result.extend([a, b]); continue;
        }
        if depth >= 38 || b - a <= 1e-10 {
            result.push((a + b) * 0.5); continue;
        }
        let (left, right) = part.split(0.5); let middle = (a + b) * 0.5;
        stack.push((right, middle, b, depth + 1)); stack.push((left, a, middle, depth + 1));
        // Tangencies with a wide tolerance band should not create exponentially
        // many indistinguishable roots.
        if max - min <= tolerance && (max + min).abs() <= tolerance * 2.0 {
            stack.pop(); stack.pop(); result.push(middle);
        }
    }
    unique(&mut result); result
}

fn inside_wire(wire: &Wire, plane: Plane, point: [f64; 3]) -> bool {
    let Some(p) = plane.project(point) else { return false; };
    let mut winding = false;
    let mut previous = plane.project(wire.point(1.0)).unwrap();
    for span in &wire.spans {
        for i in 1..=24 {
            let current = plane.project(span.curve.point(i as f64 / 24.0)).unwrap();
            if (current[1] > p[1]) != (previous[1] > p[1])
                && p[0] < (previous[0] - current[0]) * (p[1] - current[1]) / (previous[1] - current[1]) + current[0] { winding = !winding; }
            previous = current;
        }
    }
    winding
}

fn constraint(curves: &[Curve3], sections: &[Section], guide: bool, periodic: bool) -> Result<Constraint, LoftError> {
    let mut curve = spatial_wire(curves)?;
    let tolerance = geometry_tolerance(sections.iter().flat_map(|s| s.wires.iter()).flat_map(|w| w.spans.iter())
        .flat_map(|s| s.curve.control.iter().copied().map(project)).chain(sections.iter().filter_map(|s| s.point)), 1e-7);
    let mut parameters = Vec::new(); let mut anchors = Vec::new();
    for section in sections {
        if let Some(point) = section.point {
            let (at, d) = closest_parameter(&curve, point);
            if d > tolerance { return Err(error("Every loft guide/path must meet the point cross section.")); }
            parameters.push(at); anchors.push(0.0); continue;
        }
        let plane = section.plane.unwrap();
        let mut candidates = Vec::new();
        for span in &curve.spans {
            for root in plane_roots(&span.curve, plane, tolerance * 0.01) {
                let at = span.start + (span.end - span.start) * root;
                let point = curve.point(at);
                let (anchor, d) = closest_parameter(&section.wires[0], point);
                if guide {
                    if d <= tolerance { candidates.push((at, anchor, d)); }
                } else if d <= tolerance || (!section.wires[0].closed || (inside_wire(&section.wires[0], plane, point)
                    && !section.wires[1..].iter().any(|w| inside_wire(w, plane, point)))) {
                    candidates.push((at, anchor, distance(point, section.centre.to_array())));
                }
            }
        }
        let Some((at, anchor, _)) = candidates.into_iter().min_by(|a, b| a.2.total_cmp(&b.2)) else {
            return Err(error(if guide { "Every loft guide must intersect every cross-section boundary." } else { "The loft path must pass through every cross section." }));
        };
        parameters.push(at); anchors.push(anchor);
    }
    if periodic {
        if !curve.closed { return Err(error("A periodic loft requires closed guide and path curves.")); }
        let first = parameters[0]; curve = curve.rotated(first)?;
        for t in &mut parameters { *t = (*t - first).rem_euclid(1.0); }
        parameters[0] = 0.0;
        if parameters.windows(2).any(|p| p[1] <= p[0] + 1e-9) {
            let reverse = parameters.iter().enumerate().map(|(i, t)| if i == 0 { 0.0 } else { 1.0 - t }).collect::<Vec<_>>();
            if reverse.windows(2).all(|p| p[1] > p[0] + 1e-9) { curve = curve.reversed(); parameters = reverse; }
        }
    }
    if !periodic && parameters.windows(2).all(|p| p[1] < p[0] - 1e-9) {
        curve = curve.reversed(); for p in &mut parameters { *p = 1.0 - *p; }
    }
    if parameters.windows(2).any(|p| p[1] <= p[0] + 1e-9) {
        return Err(error("Guide/path crossings must follow the cross-section selection order without backtracking."));
    }
    Ok(Constraint { curve, parameters, anchors })
}

fn remap_guides(sections: &mut [Section], guides: &mut [Constraint]) -> Result<(), LoftError> {
    if guides.is_empty() { return Ok(()); }
    let first_profile = sections.iter().position(|s| s.point.is_none()).unwrap();
    let closed = sections[first_profile].wires[0].closed;
    if closed {
        for (index, section) in sections.iter_mut().enumerate() {
            if section.point.is_some() { continue; }
            let at = guides[0].anchors[index];
            section.wires[0] = section.wires[0].rotated(at)?;
            for guide in guides.iter_mut() { guide.anchors[index] = (guide.anchors[index] - at).rem_euclid(1.0); }
            guides[0].anchors[index] = 0.0;
        }
    }
    let mut order = (0..guides.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| guides[*a].anchors[first_profile].total_cmp(&guides[*b].anchors[first_profile]));
    for (index, section) in sections.iter_mut().enumerate() {
        if section.point.is_some() { continue; }
        let mut mapping = vec![(0.0, 0.0)];
        for i in &order { mapping.push((guides[*i].anchors[index], guides[*i].anchors[first_profile])); }
        mapping.push((1.0, 1.0));
        mapping.dedup_by(|a, b| (a.0 - b.0).abs() <= 1e-8 && (a.1 - b.1).abs() <= 1e-8);
        if mapping.windows(2).any(|p| p[1].0 <= p[0].0 + 1e-10 || p[1].1 <= p[0].1 + 1e-10) {
            return Err(error("Loft guides cross or meet a section in an inconsistent perimeter order."));
        }
        let wire = &section.wires[0];
        let mut breaks = wire.spans.iter().map(|s| s.start).chain(std::iter::once(1.0)).collect::<Vec<_>>();
        breaks.extend(mapping.iter().map(|p| p.0)); unique(&mut breaks);
        let mapped = |t: f64| {
            let p = mapping.windows(2).find(|p| t <= p[1].0 + 1e-10).unwrap();
            p[0].1 + (t - p[0].0) / (p[1].0 - p[0].0) * (p[1].1 - p[0].1)
        };
        let spans = breaks.windows(2).map(|p| Ok(Span { start: mapped(p[0]), end: mapped(p[1]), curve: wire.part(p[0], p[1])? })).collect::<Result<Vec<_>, LoftError>>()?;
        section.wires[0] = Wire { spans, closed };
    }
    Ok(())
}

fn validate_point_guides(sections: &[Section], guides: &[Constraint], options: LoftOptions) -> Result<(), LoftError> {
    for index in [0, sections.len() - 1] {
        if sections[index].point.is_none() { continue; }
        let continuity = if index == 0 { options.start_continuity } else { options.end_continuity };
        if continuity == 0 { continue; }
        let neighbour = if index == 0 { 1 } else { index - 1 };
        let axis = (sections[neighbour].centre - sections[index].centre).normalize().ok_or_else(|| error("The point tangent plane is undefined for coincident sections."))?;
        for guide in guides {
            let at = guide.parameters[index];
            let span = guide.curve.spans.iter().find(|s| at <= s.end + 1e-10).unwrap();
            let local = ((at - span.start) / (span.end - span.start)).clamp(0.0, 1.0);
            let tangent = span.curve.tangent(local).normalize().or_else(|| {
                span.curve.tangent(if index == 0 { (local + 1e-6).min(1.0) } else { (local - 1e-6).max(0.0) }).normalize()
            }).ok_or_else(|| error("A loft guide has an undefined tangent at the point section."))?;
            if tangent.dot(axis).abs() > 1e-6 {
                return Err(error("A guide tangent conflicts with G1 continuity at the point. Change its tangent or use G0 continuity."));
            }
        }
    }
    Ok(())
}

fn derivative(values: &[H], index: usize, lengths: &[f64], periodic: bool) -> H {
    let last = values.len() - 1;
    if !periodic && index == 0 { return std::array::from_fn(|k| (values[1][k] - values[0][k]) / lengths[0]); }
    if !periodic && index == last { return std::array::from_fn(|k| (values[last][k] - values[last - 1][k]) / lengths[last - 1]); }
    let before = if index == 0 { last } else { index - 1 }; let after = (index + 1) % values.len();
    let a = lengths[before]; let b = lengths[index];
    let mut result: H = std::array::from_fn(|k| ((values[index][k] - values[before][k]) / a * b + (values[after][k] - values[index][k]) / b * a) / (a + b));
    // Positive source weights must not turn negative between sections. A
    // monotone weight derivative bounds the homogeneous interpolation while
    // retaining its spatial tangent and C1 continuity at the section.
    let left = (values[index][3] - values[before][3]) / a;
    let right = (values[after][3] - values[index][3]) / b;
    let weight_derivative = if left * right <= 0.0 { 0.0 } else {
        let first = 2.0 * b + a; let second = b + 2.0 * a;
        (first + second) / (first / left + second / right)
    };
    let point = project(values[index]);
    for k in 0..3 { result[k] += point[k] * (weight_derivative - result[3]); }
    result[3] = weight_derivative;
    result
}

fn section_derivative(values: &[H], index: usize, sections: &[Section], lengths: &[f64], options: LoftOptions) -> H {
    let mut tangent = derivative(values, index, lengths, options.closed);
    let last = values.len() - 1;
    if sections[index].point.is_some() {
        let start = index == 0;
        let continuity = if start { options.start_continuity } else { options.end_continuity };
        let bulge = if start { options.start_bulge } else { options.end_bulge };
        let neighbour = if start { 1 } else { index - 1 };
        let width = if start { lengths[0] } else { lengths[index - 1] };
        let point = Vec3::from(project(values[index]));
        let mut spatial = Vec3::new((tangent[0] - point.x * tangent[3]) / values[index][3],
            (tangent[1] - point.y * tangent[3]) / values[index][3], (tangent[2] - point.z * tangent[3]) / values[index][3]);
        if continuity == 1 {
            let axis = (sections[neighbour].centre - sections[index].centre).normalize().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
            let radial = Vec3::from(project(values[neighbour])) - sections[neighbour].centre;
            let radial = radial - axis * radial.dot(axis);
            spatial = radial * ((if start { 3.0 } else { -3.0 }) * bulge / width);
        } else { spatial = spatial * (2.0 * bulge); }
        tangent[0] = spatial.x * values[index][3] + point.x * tangent[3];
        tangent[1] = spatial.y * values[index][3] + point.y * tangent[3];
        tangent[2] = spatial.z * values[index][3] + point.z * tangent[3];
        return tangent;
    }
    let normal = match options.normals {
        2 => index == 0, 3 => index == last, 4 => index == 0 || index == last, 5 => true,
        6 => index == 0 || index == last, _ => false,
    };
    if normal {
        if let Some(plane) = sections[index].plane {
            let h = values[index]; let point = Vec3::from(project(h));
            let spatial = Vec3::new((tangent[0] - point.x * tangent[3]) / h[3], (tangent[1] - point.y * tangent[3]) / h[3], (tangent[2] - point.z * tangent[3]) / h[3]);
            let mut direction = Vec3::from(plane.normal().unwrap());
            let before = if index > 0 { index - 1 } else if options.closed { last } else { 0 };
            let after = if index < last { index + 1 } else if options.closed { 0 } else { last };
            if direction.dot(sections[after].centre - sections[before].centre) < 0.0 { direction = -direction; }
            let magnitude = if options.normals == 6 {
                if index == 0 { options.start_magnitude } else { options.end_magnitude }
            } else { 0.0 };
            let speed = if magnitude > 0.0 {
                // Magnitudes are tangent lengths, independent of the section
                // chord-length parameter used internally by interpolation.
                magnitude / if index == 0 { lengths[0] } else { lengths[last - 1] }
            } else { spatial.length().max(1e-9) };
            if options.normals == 6 {
                let angle = if index == 0 { options.start_draft_angle } else { options.end_draft_angle };
                let radial = point - sections[index].centre;
                let radial = (radial - direction * radial.dot(direction)).normalize().unwrap_or(Vec3::ZERO);
                direction = direction * angle.sin() + radial * angle.cos();
            }
            let d = direction * speed;
            tangent[0] = d.x * h[3] + point.x * tangent[3];
            tangent[1] = d.y * h[3] + point.y * tangent[3];
            tangent[2] = d.z * h[3] + point.z * tangent[3];
        }
    }
    tangent
}

fn base_patch(curves: &[Vec<Bezier>], column: usize, band: usize, sections: &[Section], lengths: &[f64], options: LoftOptions) -> Patch {
    let next = (band + 1) % curves.len(); let width = lengths[band];
    let control = (0..curves[band][column].control.len()).map(|i| {
        let mut values = curves.iter().map(|r| r[column].control[i]).collect::<Vec<_>>();
        let a = values[band]; let b = values[next];
        if options.normals == 0 && sections[band].point.is_none() && sections[next].point.is_none() { return vec![a, b]; }
        let mut open_sections = Vec::new();
        let (interpolation_sections, next, interpolation_options) = if options.closed && !options.periodic {
            values.push(values[0]); open_sections.extend_from_slice(sections); open_sections.push(sections[0].clone());
            (&open_sections[..], band + 1, LoftOptions { closed: false, ..options })
        } else { (sections, next, options) };
        let first = section_derivative(&values, band, interpolation_sections, lengths, interpolation_options);
        let last = section_derivative(&values, next, interpolation_sections, lengths, interpolation_options);
        vec![a, std::array::from_fn(|k| a[k] + first[k] * width / 3.0), std::array::from_fn(|k| b[k] - last[k] * width / 3.0), b]
    }).collect();
    Patch { control }
}

fn choose(n: usize, k: usize) -> f64 {
    let k = k.min(n - k);
    (0..k).fold(1.0, |v, i| v * (n - i) as f64 / (i + 1) as f64)
}

/// Rational Bézier addition by multiplication of Bernstein denominators.
/// This keeps guide rails exact even when their weights differ from a section.
fn patch_sum(a: &Patch, b: &Patch, sign: f64) -> Patch {
    if polynomial_weight(b).is_some() { return add_polynomial(a, b, sign); }
    if polynomial_weight(a).is_some() {
        let mut sum = add_polynomial(b, a, sign);
        for h in sum.control.iter_mut().flatten() { for value in &mut h[..3] { *value *= sign; } }
        return sum;
    }
    let (au, av, bu, bv) = (a.u_degree(), a.v_degree(), b.u_degree(), b.v_degree());
    let mut result = vec![vec![[0.0; 4]; av + bv + 1]; au + bu + 1];
    for i in 0..=au { for j in 0..=av { for k in 0..=bu { for l in 0..=bv {
        let factor = choose(au, i) * choose(bu, k) / choose(au + bu, i + k)
            * choose(av, j) * choose(bv, l) / choose(av + bv, j + l);
        let x = a.control[i][j]; let y = b.control[k][l];
        for c in 0..3 { result[i + k][j + l][c] += (x[c] * y[3] + sign * y[c] * x[3]) * factor; }
        result[i + k][j + l][3] += x[3] * y[3] * factor;
    } } } }
    Patch { control: result }
}

fn polynomial_weight(patch: &Patch) -> Option<f64> {
    let first = patch.control[0][0][3];
    patch.control.iter().flatten().all(|h| (h[3] - first).abs() <= first.abs() * 1e-12).then_some(first)
}

fn add_polynomial(a: &Patch, b: &Patch, sign: f64) -> Patch {
    // A unit (or constant) denominator has degree zero. Multiplying its
    // unnecessarily elevated representation into every guide patch inflates
    // degrees and makes interactive tessellation needlessly expensive.
    let weight = polynomial_weight(b).unwrap();
    let mut denominator = a.control.iter().map(|row| row.iter().map(|h| h[3]).collect::<Vec<_>>()).collect::<Vec<_>>();
    if denominator.iter().all(|row| row.iter().zip(&denominator[0]).all(|(a, b)| (a - b).abs() <= 1e-12 * a.abs().max(1.0))) {
        denominator.truncate(1);
    }
    if denominator.iter().all(|row| row.iter().all(|w| (*w - row[0]).abs() <= 1e-12 * w.abs().max(1.0))) {
        for row in &mut denominator { row.truncate(1); }
    }
    let (du, dv, bu, bv) = (denominator.len() - 1, denominator[0].len() - 1, b.u_degree(), b.v_degree());
    let mut product = vec![vec![[0.0; 4]; dv + bv + 1]; du + bu + 1];
    for i in 0..=du { for j in 0..=dv { for k in 0..=bu { for l in 0..=bv {
        let factor = choose(du, i) * choose(bu, k) / choose(du + bu, i + k)
            * choose(dv, j) * choose(bv, l) / choose(dv + bv, j + l) * denominator[i][j] / weight;
        for c in 0..3 { product[i + k][j + l][c] += b.control[k][l][c] * factor; }
    } } } }
    let product = Patch { control: product };
    let u = a.u_degree().max(product.u_degree()); let v = a.v_degree().max(product.v_degree());
    let mut result = a.elevated(u, v); let product = product.elevated(u, v);
    for (row, correction) in result.control.iter_mut().zip(product.control) {
        for (h, delta) in row.iter_mut().zip(correction) { for c in 0..3 { h[c] += sign * delta[c]; } }
    }
    result
}

fn curve_patch(curve: &Bezier) -> Patch { Patch { control: vec![curve.control.clone()] } }
fn curve_sum(a: &Bezier, b: &Bezier, sign: f64) -> Bezier { Bezier { control: patch_sum(&curve_patch(a), &curve_patch(b), sign).control.remove(0) } }

fn rail_correction(base: &Patch, rail: &Bezier, end: bool) -> Patch {
    let difference = curve_sum(rail, &base.u_edge(end), -1.0);
    let zero = difference.control.iter().map(|h| [0.0, 0.0, 0.0, h[3]]).collect();
    Patch { control: if end { vec![zero, difference.control] } else { vec![difference.control, zero] } }
}

fn constraint_interval(constraint: &Constraint, band: usize, periodic: bool) -> (f64, f64) {
    (constraint.parameters[band], if band + 1 < constraint.parameters.len() { constraint.parameters[band + 1] } else if periodic { 1.0 } else { constraint.parameters[band] })
}

fn band_breaks(constraints: &[&Constraint], band: usize, periodic: bool) -> Vec<f64> {
    let mut breaks = vec![0.0, 1.0];
    for constraint in constraints {
        let (a, b) = constraint_interval(constraint, band, periodic);
        for span in &constraint.curve.spans {
            if span.start > a + 1e-10 && span.start < b - 1e-10 { breaks.push((span.start - a) / (b - a)); }
        }
    }
    unique(&mut breaks); breaks
}

fn path_shift(path: &Constraint, band: usize, a: f64, b: f64, sections: &[Section], lengths: &[f64], options: LoftOptions) -> Result<Bezier, LoftError> {
    let (start, end) = constraint_interval(path, band, options.closed);
    let mut exact = path.curve.part(start + (end - start) * a, start + (end - start) * b)?;
    if band == 0 && a <= 1e-10 && sections[0].point.is_some() && options.start_continuity == 1 { exact = exact.eased_endpoint(true); }
    if band + 2 == sections.len() && b >= 1.0 - 1e-10 && sections.last().unwrap().point.is_some() && options.end_continuity == 1 { exact = exact.eased_endpoint(false); }
    let mut values = path.parameters.iter().map(|t| homogeneous(path.curve.point(*t), 1.0)).collect::<Vec<_>>();
    let next = (band + 1) % values.len();
    let base = if options.normals == 0 && sections[band].point.is_none() && sections[next].point.is_none() { Bezier { control: vec![values[band], values[next]] } } else {
        let mut open_sections = Vec::new();
        let (interpolation_sections, next, interpolation_options) = if options.closed && !options.periodic {
            values.push(values[0]); open_sections.extend_from_slice(sections); open_sections.push(sections[0].clone());
            (&open_sections[..], band + 1, LoftOptions { closed: false, ..options })
        } else { (sections, next, options) };
        let first = section_derivative(&values, band, interpolation_sections, lengths, interpolation_options);
        let last = section_derivative(&values, next, interpolation_sections, lengths, interpolation_options);
        Bezier { control: vec![values[band], std::array::from_fn(|i| values[band][i] + first[i] * lengths[band] / 3.0),
            std::array::from_fn(|i| values[next][i] - last[i] * lengths[band] / 3.0), values[next]] }
    }.part(a, b);
    Ok(curve_sum(&exact, &base, -1.0))
}

/// Builds a solid or sheet while retaining exact input boundaries. Open
/// profiles always produce a sheet, even when `surface` was not requested.
pub fn loft_with_options(sections: &[LoftSection], guides: &[Vec<Curve3>], path: Option<&[Curve3]>, options: LoftOptions) -> Result<Body, LoftError> {
    if !guides.is_empty() && path.is_some() { return Err(error("Choose either loft guides or a loft path, not both.")); }
    let prepared_sections = prepared(sections, options)?;
    if guides.is_empty() && path.is_none() {
        if let Some(body) = circular_pair(sections, &prepared_sections, options) { return Ok(body); }
    }
    let mut sections = prepared_sections;
    let mut guides = guides.iter().map(|g| constraint(g, &sections, true, options.closed)).collect::<Result<Vec<_>, _>>()?;
    let path = path.map(|p| constraint(p, &sections, false, options.closed)).transpose()?;
    validate_point_guides(&sections, &guides, options)?;
    remap_guides(&mut sections, &mut guides)?;
    let first_profile = sections.iter().position(|s| s.point.is_none()).unwrap();
    let count = sections[first_profile].wires.len();
    let profile_closed = sections[first_profile].wires[0].closed;
    let solid = profile_closed && !options.surface;
    let bands = sections.len() - usize::from(!options.closed);
    let mut lengths = Vec::new();
    for band in 0..bands {
        let next = (band + 1) % sections.len();
        let mut length = (sections[next].centre - sections[band].centre).length();
        if length < 1e-9 && sections[band].point.is_none() && sections[next].point.is_none() {
            length = (0..16).map(|i| distance(sections[band].wires[0].point(i as f64 / 16.0), sections[next].wires[0].point(i as f64 / 16.0))).sum::<f64>() / 16.0;
        }
        if length <= 1e-10 { return Err(error("Adjacent loft cross sections coincide.")); }
        lengths.push(length);
    }
    let constraints = guides.iter().chain(path.iter()).collect::<Vec<_>>();
    let mut grids = Vec::new();
    for wire_index in 0..count {
        let mut breaks = vec![0.0, 1.0];
        for section in &sections { if section.point.is_none() { breaks.extend(section.wires[wire_index].spans.iter().map(|s| s.start)); } }
        unique(&mut breaks);
        let mut curves = Vec::new();
        for section in &sections {
            let wire = if section.point.is_some() { &sections[first_profile].wires[wire_index] } else { &section.wires[wire_index] };
            let mut row = Vec::new();
            for pair in breaks.windows(2) {
                let mut curve = wire.part(pair[0], pair[1])?;
                if let Some(point) = section.point { for h in &mut curve.control { *h = homogeneous(point, h[3]); } }
                row.push(curve.unit_end_weights());
            }
            curves.push(row);
        }
        for column in 0..breaks.len() - 1 {
            let degree = curves.iter().map(|r| r[column].degree()).max().unwrap();
            for row in &mut curves { row[column] = row[column].elevated(degree); }
        }
        let mut grid = Vec::new();
        for band in 0..bands {
            let longitudinal = band_breaks(&constraints, band, options.closed);
            let base = (0..breaks.len() - 1).map(|column| base_patch(&curves, column, band, &sections, &lengths, options)).collect::<Vec<_>>();
            for pair in longitudinal.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let shift = path.as_ref().map(|path| path_shift(path, band, a, b, &sections, &lengths, options)).transpose()?;
                let mut row = Vec::new();
                for (column, source) in base.iter().enumerate() {
                    let base = source.v_part(a, b);
                    let mut patch = base.clone();
                    if let Some(shift) = &shift { patch = patch_sum(&patch, &curve_patch(shift), 1.0); }
                    if wire_index == 0 {
                        for (end, u) in [(false, breaks[column]), (true, breaks[column + 1])] {
                            let u = if profile_closed && u >= 1.0 - 1e-10 { 0.0 } else { u };
                            if let Some(guide) = guides.iter().find(|g| (g.anchors[first_profile] - u).abs() <= 1e-8) {
                                let (start, finish) = constraint_interval(guide, band, options.closed);
                                let rail = guide.curve.part(start + (finish - start) * a, start + (finish - start) * b)?;
                                patch = patch_sum(&patch, &rail_correction(&base, &rail, end), 1.0);
                            }
                        }
                    }
                    // Validate weights before topological assembly, so a failed
                    // interpolation never returns a partial or empty body.
                    patch.surface()?;
                    row.push(patch);
                }
                grid.push(row);
            }
        }
        grids.push(grid);
    }
    assemble(&sections, &grids, profile_closed, solid, options.closed)
}

/// Two untwisted coaxial circles interpolate to an analytic cone or cylinder.
/// Retain that identity instead of replacing editable circular cap boundaries
/// with spline edges. Raw arcs prove circularity; homogeneous control data,
/// after the ordinary seam alignment, proves the loft has straight generators.
fn circular_pair(source: &[LoftSection], sections: &[Section], options: LoftOptions) -> Option<Body> {
    if source.len() != 2 || options.closed || !matches!(options.normals, 0 | 1)
        || options.start_draft_angle != FRAC_PI_2 || options.end_draft_angle != FRAC_PI_2
        || options.start_magnitude != 0.0 || options.end_magnitude != 0.0 { return None; }
    let (first_plane, first_centre, first_radius) = circular_section(&source[0])?;
    let (last_plane, last_centre, last_radius) = circular_section(&source[1])?;
    let axis = Vec3::from(first_plane.normal()?);
    let delta = last_centre - first_centre;
    let height = delta.dot(axis);
    let tolerance = geometry_tolerance([first_centre.to_array(), last_centre.to_array()], f64::EPSILON * 128.0)
        .max(first_radius.max(last_radius) * f64::EPSILON * 128.0);
    if height.abs() <= tolerance.max(1e-10) || (delta - axis * height).length() > tolerance
        || axis.cross(Vec3::from(last_plane.normal()?)).length() > f64::EPSILON * 128.0 { return None; }
    let first = &sections[0].wires[0];
    let last = &sections[1].wires[0];
    if first.spans.len() != last.spans.len() { return None; }
    for (a, b) in first.spans.iter().zip(&last.spans) {
        if (a.start - b.start).abs() > f64::EPSILON * 128.0
            || (a.end - b.end).abs() > f64::EPSILON * 128.0
            || a.curve.control.len() != b.curve.control.len() { return None; }
        for (a, b) in a.curve.control.iter().zip(&b.curve.control) {
            if (a[3] - b[3]).abs() > a[3].abs().max(b[3].abs()) * f64::EPSILON * 128.0 { return None; }
            let expected = last_centre + (Vec3::from(project(*a)) - first_centre) * (last_radius / first_radius);
            if expected.distance(Vec3::from(project(*b))) > tolerance { return None; }
        }
    }
    let radial = (Vec3::from(first.point(0.0)) - first_centre).normalize()?;
    let plane = Plane::from_axes(first_centre.to_array(), radial.to_array(), axis.to_array());
    if options.surface {
        return super::revolve_surface(plane, &[Curve::Line(Line {
            start: [first_radius, 0.0], end: [last_radius, height],
        })], first_centre.to_array(), axis.to_array(), TAU);
    }
    let points = [[0.0, 0.0], [first_radius, 0.0], [last_radius, height], [0.0, height]];
    let profile = (0..4).map(|i| Curve::Line(Line { start: points[i], end: points[(i + 1) % 4] })).collect::<Vec<_>>();
    super::revolve(plane, &profile, first_centre.to_array(), axis.to_array(), TAU)
}

fn circular_section(section: &LoftSection) -> Option<(Plane, Vec3, f64)> {
    let LoftSection::Profile { plane, wires, closed: true } = section else { return None; };
    if wires.len() != 1 || wires[0].is_empty() { return None; }
    let x = Vec3::from(plane.x_axis); let y = Vec3::from(plane.y_axis);
    // A sheared or unequally scaled plane maps a local circle to an ellipse.
    let angular_tolerance = f64::EPSILON * 128.0;
    if (x.length_squared() - 1.0).abs() > angular_tolerance
        || (y.length_squared() - 1.0).abs() > angular_tolerance
        || x.dot(y).abs() > angular_tolerance { return None; }
    let Curve::Arc(first) = wires[0].first()? else { return None; };
    if !first.radius.is_finite() || first.radius <= 0.0 { return None; }
    let mut sweep = 0.0;
    for (index, curve) in wires[0].iter().enumerate() {
        let Curve::Arc(arc) = curve else { return None; };
        let Curve::Arc(next) = &wires[0][(index + 1) % wires[0].len()] else { return None; };
        if arc.centre != first.centre || arc.radius != first.radius { return None; }
        let gap = (arc.end_angle - next.start_angle).rem_euclid(TAU);
        if gap.min(TAU - gap) > angular_tolerance { return None; }
        sweep += arc.sweep();
    }
    if (sweep - TAU).abs() > angular_tolerance { return None; }
    Some((*plane, Vec3::from(plane.point_at(first.centre)), first.radius))
}

fn add_vertex(body: &mut Body, point: [f64; 3]) -> VertexKey {
    body.vertices.insert(Vertex { point, provenance: Provenance::Synthesized })
}

fn add_edge(body: &mut Body, curve: &Bezier, start: VertexKey, end: VertexKey) -> Result<EdgeKey, LoftError> {
    let curve = curve.rational().curve().ok_or_else(|| error("A loft boundary contains invalid rational geometry."))?;
    let curve = body.curves.insert(Curve3::Nurbs(curve));
    Ok(body.edges.insert(Edge { curve, start_parameter: 0.0, end_parameter: 1.0, start, end, coedges: Vec::new(), provenance: Provenance::Synthesized }))
}

fn add_loop(body: &mut Body, face: super::FaceKey, circuit: &[(EdgeKey, bool, Option<([f64; 2], [f64; 2])>)]) -> Result<(), LoftError> {
    let ring = body.loops.insert(Loop { coedges: Vec::new(), owner: face, provenance: Provenance::Synthesized });
    let mut coedges = Vec::new();
    for (edge, forward, pcurve) in circuit {
        let coedge = body.coedges.insert(Coedge { edge: *edge, forward: *forward,
            pcurve: pcurve.map(|(start, end)| Curve::Line(Line { start, end })), owner: ring, provenance: Provenance::Synthesized });
        body.edges.get_mut(*edge).ok_or_else(|| error("Loft boundary ownership is invalid."))?.coedges.push(coedge);
        coedges.push(coedge);
    }
    body.loops.get_mut(ring).unwrap().coedges = coedges;
    body.faces.get_mut(face).unwrap().loops.push(ring);
    Ok(())
}

fn winding(wire: &Wire, normal: Vec3) -> f64 {
    let centre = wire_centre(wire);
    let mut area = Vec3::ZERO; let mut previous = Vec3::from(wire.point(0.0)) - centre;
    for span in &wire.spans {
        for i in 1..=12 {
            let current = Vec3::from(span.curve.point(i as f64 / 12.0)) - centre;
            area = area + previous.cross(current); previous = current;
        }
    }
    area.dot(normal)
}

fn add_cap(body: &mut Body, shell: ShellKey, section: &Section, rims: &[Vec<Option<EdgeKey>>], outward: Vec3) -> Result<(), LoftError> {
    if section.point.is_some() { return Ok(()); }
    let source = section.plane.unwrap();
    let normal = Vec3::from(source.normal().unwrap());
    let direction = if normal.dot(outward) >= 0.0 { normal } else { -normal };
    let plane = Plane::orthonormal(source.origin, source.x_axis, direction.to_array()).ok_or_else(|| error("A loft cap has an invalid section plane."))?;
    let surface = body.surfaces.insert(Surface::Plane(plane));
    let face = body.faces.insert(Face { surface, forward: true, loops: Vec::new(), owner: shell, provenance: Provenance::Synthesized });
    for (index, edges) in rims.iter().enumerate() {
        let along = (winding(&section.wires[index], direction) > 0.0) == (index == 0);
        let edges = edges.iter().copied().collect::<Option<Vec<_>>>().ok_or_else(|| error("A loft cap contains a collapsed boundary."))?;
        let circuit = if along { edges.iter().map(|e| (*e, true, None)).collect::<Vec<_>>() }
            else { edges.iter().rev().map(|e| (*e, false, None)).collect() };
        add_loop(body, face, &circuit)?;
    }
    body.shells.get_mut(shell).unwrap().faces.push(face);
    Ok(())
}

fn assemble(sections: &[Section], grids: &[Vec<Vec<Patch>>], profile_closed: bool, solid: bool, periodic: bool) -> Result<Body, LoftError> {
    let mut body = Body::new();
    let lump = body.lumps.insert(Lump { shells: Vec::new(), provenance: Provenance::Synthesized });
    let shell = body.shells.insert(Shell { faces: Vec::new(), owner: lump, provenance: Provenance::Synthesized });
    let row_count = grids[0].len(); let station_count = row_count + usize::from(!periodic);
    let mut first_rims = Vec::new(); let mut last_rims = Vec::new();
    let tolerance = geometry_tolerance(grids.iter().flatten().flatten().flat_map(|p| p.control.iter().flatten()).map(|h| project(*h)), 1e-8);
    let first_profile = sections.iter().position(|s| s.point.is_none()).unwrap();
    let normal = Vec3::from(sections[first_profile].plane.unwrap().normal().unwrap());
    let travel = if first_profile == 0 { sections[1].centre - sections[0].centre }
        else { sections[first_profile].centre - sections[first_profile - 1].centre };
    for (wire_index, grid) in grids.iter().enumerate() {
        // Propagate a single orientation around the connected strip. A radial
        // centre heuristic flips reentrant walls of a concave section.
        let forward = !profile_closed || ((winding(&sections[first_profile].wires[wire_index], normal) * normal.dot(travel) >= 0.0) == (wire_index == 0));
        let columns = grid[0].len(); let vertex_count = columns + usize::from(!profile_closed);
        let mut vertices = Vec::new(); let mut rim_curves = Vec::new();
        for station in 0..station_count {
            let row = if station < row_count { &grid[station] } else { &grid[row_count - 1] };
            let curves = row.iter().map(|p| p.v_edge(station == row_count)).collect::<Vec<_>>();
            let mut points = curves.iter().map(|c| c.point(0.0)).collect::<Vec<_>>();
            if !profile_closed { points.push(curves.last().unwrap().point(1.0)); }
            let collapsed = points.iter().all(|p| distance(*p, points[0]) <= tolerance)
                && curves.iter().all(|c| distance(c.point(0.5), points[0]) <= tolerance);
            let corners = if collapsed { vec![add_vertex(&mut body, points[0]); vertex_count] }
                else { points.iter().map(|p| add_vertex(&mut body, *p)).collect::<Vec<_>>() };
            vertices.push(corners); rim_curves.push((curves, collapsed));
        }
        let mut rims = Vec::new();
        for station in 0..station_count {
            let (curves, collapsed) = &rim_curves[station];
            let mut edges = Vec::new();
            for (column, curve) in curves.iter().enumerate() {
                edges.push(if *collapsed { None } else { Some(add_edge(&mut body, curve, vertices[station][column], vertices[station][(column + 1) % vertex_count])?) });
            }
            rims.push(edges);
        }
        first_rims.push(rims[0].clone()); last_rims.push(rims[station_count - 1].clone());
        for (row_index, row) in grid.iter().enumerate() {
            let next = (row_index + 1) % station_count;
            let mut rails = Vec::new();
            for column in 0..vertex_count {
                let rail = if column < columns { row[column].u_edge(false) } else { row[columns - 1].u_edge(true) };
                rails.push(add_edge(&mut body, &rail, vertices[row_index][column], vertices[next][column])?);
            }
            for (column, patch) in row.iter().enumerate() {
                let next_column = (column + 1) % vertex_count;
                // Shared edges must agree geometrically, not merely share keys.
                let mut boundaries = vec![(rails[column], patch.u_edge(false)), (rails[next_column], patch.u_edge(true))];
                if let Some(edge) = rims[row_index][column] { boundaries.push((edge, patch.v_edge(false))); }
                if let Some(edge) = rims[next][column] { boundaries.push((edge, patch.v_edge(true))); }
                for (edge, candidate) in boundaries {
                    let source = body.edges.get(edge).unwrap(); let source = body.curves.get(source.curve).unwrap();
                    if [0.0, 0.25, 0.5, 0.75, 1.0].iter().any(|t| distance(source.point_at(*t), candidate.point(*t)) > tolerance) {
                        return Err(error("Adjacent loft patches disagree along a guide or seam."));
                    }
                }
                let surface = Surface::Nurbs(patch.surface()?);
                if surface.normal_at(0.5, 0.5).is_none() { return Err(error("A loft patch is locally degenerate.")); }
                let surface = body.surfaces.insert(surface);
                let face = body.faces.insert(Face { surface, forward, loops: Vec::new(), owner: shell, provenance: Provenance::Synthesized });
                let mut circuit = Vec::new();
                if let Some(edge) = rims[row_index][column] { circuit.push((edge, true, Some(([0.0, 0.0], [1.0, 0.0])))); }
                circuit.push((rails[next_column], true, Some(([1.0, 0.0], [1.0, 1.0]))));
                if let Some(edge) = rims[next][column] { circuit.push((edge, false, Some(([1.0, 1.0], [0.0, 1.0])))); }
                circuit.push((rails[column], false, Some(([0.0, 1.0], [0.0, 0.0]))));
                if !forward {
                    circuit.reverse();
                    for (_, along, pcurve) in &mut circuit {
                        *along = !*along;
                        if let Some((start, end)) = pcurve { std::mem::swap(start, end); }
                    }
                }
                add_loop(&mut body, face, &circuit)?;
                body.shells.get_mut(shell).unwrap().faces.push(face);
            }
        }
    }
    if solid && !periodic {
        add_cap(&mut body, shell, &sections[0], &first_rims, sections[0].centre - sections[1].centre)?;
        let last = sections.len() - 1;
        add_cap(&mut body, shell, &sections[last], &last_rims, sections[last].centre - sections[last - 1].centre)?;
    }
    body.lumps.get_mut(lump).unwrap().shells.push(shell); body.roots.push(lump);
    let flaws = body.validate();
    if !flaws.is_empty() { return Err(LoftError(format!("Loft topology is invalid: {:?}", flaws[0]))); }
    if solid && body.edges.iter().any(|(_, edge)| edge.coedges.len() != 2) {
        return Err(error("The loft solid has an unsealed boundary."));
    }
    Ok(body)
}
