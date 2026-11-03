//! Revision clouds: the chain of equal arcs REVCLOUD lays along a path.
//!
//! A cloud is a lightweight polyline whose arcs all sweep 110° and bulge
//! outward. Its vertices sit at path distances one chord apart — exactly the
//! arc length when REVCLOUDARCVARIANCE is off, a random length between two
//! thirds and four thirds of it when on — so corners of the path are cut
//! rather than kept. The arc length it was made with is kept in the
//! `RevcloudProps` xdata (1070 0, 1040 arc length), as the reference writes
//! it, and the path is read back from the vertices for grip edits.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use codec::entities::{LwPolyline, LwVertex};
use codec::xdata::{ExtendedDataRecord, XDataValue};
use glam::DVec3;
use kernel::geom2d::{closest_point, signed_area, Curve, Polyline, PolylineVertex, Vec2};

use crate::entities::common::square_grip;
use crate::scene::model::object::{GripApply, GripDef};

/// The xdata application every revision cloud carries.
pub(crate) const APP: &str = "RevcloudProps";
/// REVCLOUDAPPROXARCLEN, kept in the drawing's variable dictionary.
pub(crate) const ARC_LENGTH_VARIABLE: &str = "#REVCLOUDAPPROXARCLEN";
const MAX_VERTICES: usize = 100_000;

/// The bulge of every cloud arc: a 110° sweep.
pub(crate) fn arc_bulge() -> f64 {
    27.5_f64.to_radians().tan()
}

static CREATE_MODE: AtomicU8 = AtomicU8::new(1);
static ARC_VARIANCE: AtomicBool = AtomicBool::new(true);
static GRIPS: AtomicBool = AtomicBool::new(true);

/// REVCLOUDCREATEMODE: 0 Freehand, 1 Rectangular, 2 Polygonal.
pub fn create_mode() -> u8 {
    CREATE_MODE.load(Ordering::Relaxed).min(2)
}

pub fn set_create_mode(mode: u8) {
    CREATE_MODE.store(mode.min(2), Ordering::Relaxed);
}

/// REVCLOUDARCVARIANCE: chords of random length (on) or all one length.
pub fn arc_variance() -> bool {
    ARC_VARIANCE.load(Ordering::Relaxed)
}

pub fn set_arc_variance(on: bool) {
    ARC_VARIANCE.store(on, Ordering::Relaxed);
}

/// REVCLOUDGRIPS: a cloud shows the grips of its path (on) or of every
/// vertex.
pub fn grips_on() -> bool {
    GRIPS.load(Ordering::Relaxed)
}

pub fn set_grips_on(on: bool) {
    GRIPS.store(on, Ordering::Relaxed);
}

/// The shortest and longest chord for an arc length.
pub(crate) fn chord_range(arc_length: f64, variance: bool) -> (f64, f64) {
    if variance {
        (arc_length * 2.0 / 3.0, arc_length * 4.0 / 3.0)
    } else {
        (arc_length, arc_length)
    }
}

/// The arc length REVCLOUD offers while REVCLOUDAPPROXARCLEN is 0: a
/// twentieth of the current view's diagonal.
// ponytail: the reference's factor drifts by 0.1% with the window aspect;
// 0.05 of the diagonal is the measured mean.
pub(crate) fn view_arc_length(view_height: f64, aspect: f64) -> f64 {
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
    0.05 * view_height * (1.0 + aspect * aspect).sqrt()
}

/// A uniform number in `[0, 1)`.
fn random_unit() -> f64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        let mut seed = [0_u8; 8];
        let _ = getrandom::fill(&mut seed);
        x = u64::from_le_bytes(seed) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1_u64 << 53) as f64
}

/// The arc length a cloud was made with: its xdata, else the mean chord of
/// a cloud written without one.
pub(crate) fn arc_length_of(pline: &LwPolyline) -> Option<f64> {
    if let Some(value) = pline
        .common
        .extended_data
        .get_record(APP)
        .and_then(|record| {
            record.values.iter().find_map(|value| match value {
                XDataValue::Real(length) => Some(*length),
                _ => None,
            })
        })
        .filter(|length| length.is_finite() && *length > 0.0)
    {
        return Some(value);
    }
    if !looks_like_cloud(pline) {
        return None;
    }
    let total = straight_guide(pline).length();
    (total.is_finite() && total > 0.0).then_some(total / pline.vertices.len() as f64)
}

/// A cloud: the `RevcloudProps` xdata, or (a file written without it) a
/// closed chain of arcs bulging one way with one sweep.
pub(crate) fn is_cloud(pline: &LwPolyline) -> bool {
    pline.common.extended_data.get_record(APP).is_some() || looks_like_cloud(pline)
}

fn looks_like_cloud(pline: &LwPolyline) -> bool {
    if !pline.is_closed || pline.vertices.len() < 3 {
        return false;
    }
    let first = pline.vertices[0].bulge;
    first.is_finite()
        && first.abs() > 1.0e-9
        && pline.vertices.iter().all(|vertex| {
            vertex.bulge.signum() == first.signum() && (vertex.bulge.abs() - first.abs()).abs() <= 0.15
        })
        && (0.35..=0.65).contains(&first.abs())
}

/// Store the arc length in the cloud's xdata.
pub(crate) fn write_props(pline: &mut LwPolyline, arc_length: f64) {
    let mut record = ExtendedDataRecord::new(APP);
    record.add_value(XDataValue::Integer16(0));
    record.add_value(XDataValue::Real(arc_length));
    pline.common.extended_data.upsert_record(record);
}

/// Path distances of the cloud's vertices along a path of `length`.
fn stations(length: f64, closed: bool, arc_length: f64, variance: bool) -> Option<Vec<f64>> {
    if !length.is_finite() || length <= 0.0 || !arc_length.is_finite() || arc_length <= 0.0 {
        return None;
    }
    if length / arc_length > MAX_VERTICES as f64 * 0.75 {
        return None;
    }
    let tolerance = length * 1.0e-9;
    let mut out = vec![0.0];
    if !variance && closed {
        // Whole chords; a remainder under half a chord joins the last one.
        let count = (length / arc_length).round() as usize;
        if count < 3 {
            return Some(vec![0.0, length / 3.0, length * 2.0 / 3.0]);
        }
        out.extend((1..count).map(|k| k as f64 * arc_length));
        return Some(out);
    }
    let (low, high) = chord_range(arc_length, variance);
    let mut distance = 0.0;
    loop {
        distance += low + (high - low) * random_unit();
        if distance >= length - tolerance {
            break;
        }
        out.push(distance);
    }
    if closed {
        if out.len() < 3 {
            out = vec![0.0, length / 3.0, length * 2.0 / 3.0];
        }
    } else {
        out.push(length);
    }
    Some(out)
}

/// A cloud laid along `curve`, starting at its start. Arcs bulge outward
/// (the side a closed path's area does not lie on; an unclosed path is
/// closed by a straight line for this), or inward when `reverse`.
pub(crate) fn cloud_along(
    curve: &Curve,
    arc_length: f64,
    variance: bool,
    calligraphy: bool,
    reverse: bool,
) -> Option<LwPolyline> {
    let closed = curve.is_closed();
    let distances = stations(curve.length(), closed, arc_length, variance)?;
    let mut points: Vec<[f64; 2]> = distances
        .into_iter()
        .map(|distance| curve.point_at_distance(distance))
        .collect();
    let tolerance = arc_length * 1.0e-9;
    points.dedup_by(|right, left| Vec2::from(*left).distance(Vec2::from(*right)) <= tolerance);
    if closed && points.len() >= 2 && Vec2::from(points[0]).distance(Vec2::from(*points.last()?)) <= tolerance {
        points.pop();
    }
    if points.len() < 2 || (closed && points.len() < 3) {
        return None;
    }
    let outward = if signed_area(&points) > 0.0 { 1.0 } else { -1.0 };
    let sign = if reverse { -outward } else { outward };
    Some(from_points(&points, closed, sign * arc_bulge(), calligraphy, arc_length, variance))
}

fn from_points(
    points: &[[f64; 2]],
    closed: bool,
    bulge: f64,
    calligraphy: bool,
    arc_length: f64,
    variance: bool,
) -> LwPolyline {
    let width = chord_range(arc_length, variance).1 / 10.0;
    let mut cloud = LwPolyline::new();
    cloud.is_closed = closed;
    cloud.vertices = points
        .iter()
        .map(|point| {
            let mut vertex = LwVertex::from_coords(point[0], point[1]);
            vertex.bulge = bulge;
            if calligraphy {
                vertex.end_width = width;
            }
            vertex
        })
        .collect();
    write_props(&mut cloud, arc_length);
    cloud
}

/// Whether the cloud is drawn in the Calligraphy style (tapered arcs).
pub(crate) fn is_calligraphy(pline: &LwPolyline) -> bool {
    pline
        .vertices
        .iter()
        .any(|vertex| vertex.start_width > 0.0 || vertex.end_width > 0.0)
}

/// Whether the cloud's arcs bulge inward (Reverse direction).
fn is_reversed(pline: &LwPolyline) -> bool {
    let points: Vec<[f64; 2]> = pline
        .vertices
        .iter()
        .map(|vertex| [vertex.location.x, vertex.location.y])
        .collect();
    let outward = if signed_area(&points) > 0.0 { 1.0 } else { -1.0 };
    pline
        .vertices
        .first()
        .is_some_and(|vertex| vertex.bulge.signum() != outward)
}

/// The straight path through the cloud's vertices.
fn straight_guide(pline: &LwPolyline) -> Curve {
    Curve::Polyline(Polyline {
        vertices: pline
            .vertices
            .iter()
            .map(|vertex| PolylineVertex::straight([vertex.location.x, vertex.location.y]))
            .collect(),
        closed: pline.is_closed,
    })
}

/// Rebuild `pline` along `curve`, keeping its style, its side and its
/// layer and other properties.
fn rebuild(pline: &mut LwPolyline, curve: &Curve, arc_length: f64) -> bool {
    let Some(mut cloud) = cloud_along(
        curve,
        arc_length,
        arc_variance(),
        is_calligraphy(pline),
        is_reversed(pline),
    ) else {
        return false;
    };
    cloud.common = pline.common.clone();
    write_props(&mut cloud, arc_length);
    cloud.normal = pline.normal;
    cloud.elevation = pline.elevation;
    cloud.thickness = pline.thickness;
    *pline = cloud;
    true
}

/// The Arc Length property: lay the cloud again along its vertices with the
/// new arc length.
pub(crate) fn set_arc_length(pline: &mut LwPolyline, arc_length: f64) -> bool {
    if !arc_length.is_finite() || arc_length <= 0.0 || pline.vertices.len() < 2 {
        return false;
    }
    let guide = straight_guide(pline);
    rebuild(pline, &guide, arc_length)
}

/// The path a cloud was laid along, read back from its vertices: a polygon
/// whose sides run through rows of vertices in line, or a circle through all
/// of them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Guide {
    Polygon(Vec<[f64; 2]>),
    Circle { center: [f64; 2], radius: f64 },
}

pub(crate) fn guide(pline: &LwPolyline) -> Option<Guide> {
    if !pline.is_closed || pline.vertices.len() < 3 {
        return None;
    }
    let points: Vec<[f64; 2]> = pline
        .vertices
        .iter()
        .map(|vertex| [vertex.location.x, vertex.location.y])
        .collect();
    let n = points.len();
    let scale = points
        .iter()
        .flat_map(|point| point.iter().map(|value| value.abs()))
        .fold(1.0_f64, f64::max);
    if n >= 4 {
        if let Some(center) = circumcenter(points[0], points[n / 3], points[2 * n / 3]) {
            let radius = Vec2::from(points[0]).distance(Vec2::from(center));
            if points.iter().all(|point| {
                (Vec2::from(*point).distance(Vec2::from(center)) - radius).abs() <= 1.0e-7 * scale
            }) {
                return Some(Guide::Circle { center, radius });
            }
        }
    }
    // Rows of chords in one direction are sides; a lone chord between two
    // sides cuts their corner.
    let direction = |i: usize| {
        let a = points[i];
        let b = points[(i + 1) % n];
        Vec2::new(b[0] - a[0], b[1] - a[1]).normalize()
    };
    let same = |a: Option<Vec2>, b: Option<Vec2>| match (a, b) {
        (Some(a), Some(b)) => (a.x * b.y - a.y * b.x).abs() <= 1.0e-7 && a.dot(b) > 0.0,
        _ => false,
    };
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (first chord, chord count)
    let mut start = 0;
    while start < n {
        let mut count = 1;
        while start + count < n && same(direction(start), direction(start + count)) {
            count += 1;
        }
        runs.push((start, count));
        start += count;
    }
    if runs.len() > 1 && same(direction(n - 1), direction(0)) {
        let (_, tail) = runs.pop()?;
        runs[0].0 = (runs[0].0 + n - tail) % n;
        runs[0].1 += tail;
    }
    let sides: Vec<usize> = (0..runs.len()).filter(|&k| runs[k].1 >= 2).collect();
    if sides.len() < 3 {
        return None;
    }
    // Between two sides at most one connecting chord.
    for (index, &side) in sides.iter().enumerate() {
        let next = sides[(index + 1) % sides.len()];
        let gap = (next + runs.len() - side - 1) % runs.len();
        if gap > 1 {
            return None;
        }
    }
    let line = |k: usize| {
        let (first, _) = runs[k];
        (points[first], points[(first + 1) % n])
    };
    let mut corners: Vec<[f64; 2]> = sides
        .iter()
        .enumerate()
        .filter_map(|(index, &side)| {
            let previous = sides[(index + sides.len() - 1) % sides.len()];
            intersect_lines(line(previous), line(side))
        })
        .collect();
    if corners.len() != sides.len() {
        return None;
    }
    // The path starts where the cloud starts.
    let first = corners
        .iter()
        .enumerate()
        .min_by(|a, b| {
            Vec2::from(*a.1)
                .distance(Vec2::from(points[0]))
                .total_cmp(&Vec2::from(*b.1).distance(Vec2::from(points[0])))
        })?
        .0;
    corners.rotate_left(first);
    Some(Guide::Polygon(corners))
}

fn circumcenter(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 2]> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1.0e-12 {
        return None;
    }
    let sq = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    Some([
        (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d,
        (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d,
    ])
}

fn intersect_lines(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])) -> Option<[f64; 2]> {
    let (p, r) = (a.0, [a.1[0] - a.0[0], a.1[1] - a.0[1]]);
    let (q, s) = (b.0, [b.1[0] - b.0[0], b.1[1] - b.0[1]]);
    let denominator = r[0] * s[1] - r[1] * s[0];
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    let t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / denominator;
    Some([p[0] + t * r[0], p[1] + t * r[1]])
}

fn polygon_curve(corners: &[[f64; 2]]) -> Curve {
    Curve::Polyline(Polyline {
        vertices: corners.iter().copied().map(PolylineVertex::straight).collect(),
        closed: true,
    })
}

fn circle_curve(center: [f64; 2], radius: f64, start: [f64; 2]) -> Curve {
    // The cloud starts where it started: the angle of its first vertex.
    let start_angle = (start[1] - center[1]).atan2(start[0] - center[0]);
    let steps = 720;
    Curve::Polyline(Polyline {
        vertices: (0..steps)
            .map(|k| {
                let angle = start_angle + std::f64::consts::TAU * k as f64 / steps as f64;
                PolylineVertex::straight([center[0] + radius * angle.cos(), center[1] + radius * angle.sin()])
            })
            .collect(),
        closed: true,
    })
}

/// The cloud's grips: the corners of its path (or the centre and
/// quadrants of its circle) while REVCLOUDGRIPS is on and the path reads
/// back; otherwise one grip per vertex.
pub(crate) fn grips(pline: &LwPolyline) -> Vec<GripDef> {
    let at = |point: [f64; 2]| DVec3::new(point[0], point[1], pline.elevation);
    match grips_on().then(|| guide(pline)).flatten() {
        Some(Guide::Polygon(corners)) => corners
            .iter()
            .enumerate()
            .map(|(index, corner)| square_grip(index, at(*corner)))
            .collect(),
        Some(Guide::Circle { center, radius }) => {
            let mut center_grip = square_grip(0, at(center));
            center_grip.is_midpoint = true;
            std::iter::once(center_grip)
                .chain((0..4).map(|k| {
                    let angle = std::f64::consts::FRAC_PI_2 * k as f64;
                    square_grip(
                        k + 1,
                        at([center[0] + radius * angle.cos(), center[1] + radius * angle.sin()]),
                    )
                }))
                .collect()
        }
        None => pline
            .vertices
            .iter()
            .enumerate()
            .map(|(index, vertex)| square_grip(index, at([vertex.location.x, vertex.location.y])))
            .collect(),
    }
}

/// Drag one of [`grips`]: a path corner or the circle's size lays the cloud
/// again; the circle's centre moves it; a vertex grip moves that vertex.
pub(crate) fn apply_grip(pline: &mut LwPolyline, grip_id: usize, apply: GripApply) {
    let target = |old: [f64; 2]| match &apply {
        GripApply::Absolute(point) => [point.x, point.y],
        GripApply::Translate(delta) => [old[0] + delta.x, old[1] + delta.y],
    };
    let arc_length = arc_length_of(pline).unwrap_or(1.0);
    match grips_on().then(|| guide(pline)).flatten() {
        Some(Guide::Polygon(mut corners)) => {
            let Some(corner) = corners.get_mut(grip_id) else {
                return;
            };
            *corner = target(*corner);
            let curve = polygon_curve(&corners);
            rebuild(pline, &curve, arc_length);
        }
        Some(Guide::Circle { center, radius }) => {
            if grip_id == 0 {
                let moved = target(center);
                let (dx, dy) = (moved[0] - center[0], moved[1] - center[1]);
                for vertex in &mut pline.vertices {
                    vertex.location.x += dx;
                    vertex.location.y += dy;
                }
                return;
            }
            let angle = std::f64::consts::FRAC_PI_2 * (grip_id - 1) as f64;
            let quadrant = [center[0] + radius * angle.cos(), center[1] + radius * angle.sin()];
            let to = target(quadrant);
            let new_radius = Vec2::from(to).distance(Vec2::from(center));
            if new_radius <= 0.0 || !new_radius.is_finite() {
                return;
            }
            let first = [pline.vertices[0].location.x, pline.vertices[0].location.y];
            let curve = circle_curve(center, new_radius, first);
            rebuild(pline, &curve, arc_length);
        }
        None => {
            if let Some(vertex) = pline.vertices.get_mut(grip_id) {
                let moved = target([vertex.location.x, vertex.location.y]);
                vertex.location.x = moved[0];
                vertex.location.y = moved[1];
            }
        }
    }
}

/// Where a point falls on a cloud: the segment and the point itself.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OnCloud {
    pub segment: usize,
    pub point: [f64; 2],
    pub distance: f64,
}

/// The nearest place on the cloud to `point`.
pub(crate) fn nearest(pline: &LwPolyline, point: [f64; 2]) -> Option<OnCloud> {
    let n = pline.vertices.len();
    let count = if pline.is_closed { n } else { n.saturating_sub(1) };
    (0..count)
        .filter_map(|index| {
            let a = &pline.vertices[index];
            let b = &pline.vertices[(index + 1) % n];
            let segment = Curve::Polyline(Polyline {
                vertices: vec![
                    PolylineVertex {
                        position: [a.location.x, a.location.y],
                        bulge: a.bulge,
                    },
                    PolylineVertex::straight([b.location.x, b.location.y]),
                ],
                closed: false,
            });
            let closest = closest_point(&segment, point);
            closest.distance.is_finite().then_some(OnCloud {
                segment: index,
                point: closest.point,
                distance: closest.distance,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

/// The run of the cloud from `from` to `to` in the cloud's own direction:
/// the vertices with their bulges, `from` first and `to` last (bulge 0).
fn run(pline: &LwPolyline, from: OnCloud, to: OnCloud) -> Vec<([f64; 2], f64)> {
    let n = pline.vertices.len();
    let location = |index: usize| {
        let vertex = &pline.vertices[index % n];
        [vertex.location.x, vertex.location.y]
    };
    // The bulge of the part of segment `s` from `a` to `b` (both on it).
    let part = |s: usize, a: [f64; 2], b: [f64; 2]| {
        let bulge = pline.vertices[s % n].bulge;
        if bulge.abs() < 1.0e-12 {
            return 0.0;
        }
        let (start, end) = (location(s), location(s + 1));
        let (_, tail) = crate::entities::lwpolyline::split_bulges_from_point(start, a, end, bulge);
        let (head, _) = crate::entities::lwpolyline::split_bulges_from_point(a, b, end, tail);
        head
    };
    let ahead = |s: usize, a: [f64; 2], b: [f64; 2]| {
        let start = Vec2::from(location(s));
        start.distance(Vec2::from(a)) <= start.distance(Vec2::from(b))
    };
    let mut out = Vec::new();
    if from.segment == to.segment && ahead(from.segment, from.point, to.point) {
        out.push((from.point, part(from.segment, from.point, to.point)));
    } else {
        out.push((from.point, part(from.segment, from.point, location(from.segment + 1))));
        let mut segment = (from.segment + 1) % n;
        for _ in 0..n {
            if segment == to.segment {
                out.push((location(segment), part(segment, location(segment), to.point)));
                break;
            }
            out.push((location(segment), pline.vertices[segment].bulge));
            segment = (segment + 1) % n;
        }
    }
    out.push((to.point, 0.0));
    out.dedup_by(|b, a| Vec2::from(a.0).distance(Vec2::from(b.0)) < 1.0e-9);
    out
}

/// REVCLOUD Modify: replace the part of a closed cloud between `start` and
/// `end` on the side of `erase` by a new cloud along `path` (from `start`
/// to `end`). The new arcs bulge to the cloud's side; the result starts at
/// the end of the kept part.
pub(crate) fn modified(
    pline: &LwPolyline,
    start: OnCloud,
    path: &[[f64; 2]],
    end: OnCloud,
    erase: OnCloud,
    arc_length: f64,
) -> Option<LwPolyline> {
    if !pline.is_closed || path.len() < 2 {
        return None;
    }
    let forward = run(pline, start, end);
    let backward = run(pline, end, start);
    let on = |part: &[([f64; 2], f64)]| {
        part.windows(2).any(|pair| {
            let segment = Curve::Polyline(Polyline {
                vertices: vec![
                    PolylineVertex {
                        position: pair[0].0,
                        bulge: pair[0].1,
                    },
                    PolylineVertex::straight(pair[1].0),
                ],
                closed: false,
            });
            closest_point(&segment, erase.point).distance <= 1.0e-6 * arc_length.max(1.0)
        })
    };
    // Keep the other part; it runs from `kept_from` to `kept_to`.
    let (kept, new_path): (Vec<([f64; 2], f64)>, Vec<[f64; 2]>) = if on(&forward) {
        (backward, path.to_vec())
    } else {
        (forward, path.iter().rev().copied().collect())
    };
    let sign = pline.vertices.first().map_or(1.0, |vertex| vertex.bulge.signum());
    let new_curve = Curve::Polyline(Polyline {
        vertices: new_path.iter().copied().map(PolylineVertex::straight).collect(),
        closed: false,
    });
    let distances = stations(new_curve.length(), false, arc_length, arc_variance())?;
    let new_points: Vec<[f64; 2]> = distances
        .into_iter()
        .map(|distance| new_curve.point_at_distance(distance))
        .collect();
    let width = if is_calligraphy(pline) {
        chord_range(arc_length, arc_variance()).1 / 10.0
    } else {
        0.0
    };
    let mut cloud = pline.clone();
    cloud.vertices.clear();
    let push = |cloud: &mut LwPolyline, point: [f64; 2], bulge: f64| {
        let mut vertex = LwVertex::from_coords(point[0], point[1]);
        vertex.bulge = bulge;
        vertex.end_width = if bulge != 0.0 { width } else { 0.0 };
        cloud.vertices.push(vertex);
    };
    // The new part from the end of the kept part back to its start ...
    for point in &new_points[..new_points.len() - 1] {
        push(&mut cloud, *point, sign * arc_bulge());
    }
    // ... then the kept part, its own arcs as they were.
    for (point, bulge) in &kept[..kept.len() - 1] {
        let mut vertex = LwVertex::from_coords(point[0], point[1]);
        vertex.bulge = *bulge;
        if let Some(original) = pline.vertices.iter().find(|vertex| {
            Vec2::new(vertex.location.x, vertex.location.y).distance(Vec2::from(*point)) < 1.0e-9
        }) {
            vertex.start_width = original.start_width;
            vertex.end_width = original.end_width;
        } else {
            vertex.end_width = width;
        }
        cloud.vertices.push(vertex);
    }
    cloud.vertices.dedup_by(|b, a| {
        Vec2::new(a.location.x, a.location.y).distance(Vec2::new(b.location.x, b.location.y)) < 1.0e-9
    });
    (cloud.vertices.len() >= 3).then_some(cloud)
}

/// Flip the side every arc bulges to.
pub(crate) fn reversed(mut pline: LwPolyline) -> LwPolyline {
    for vertex in &mut pline.vertices {
        vertex.bulge = -vertex.bulge;
    }
    pline
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle(arc: f64) -> LwPolyline {
        let curve = polygon_curve(&[[0.0, 0.0], [0.0, 60.0], [100.0, 60.0], [100.0, 0.0]]);
        cloud_along(&curve, arc, false, false, false).unwrap()
    }

    #[test]
    fn uniform_chords_follow_the_path_and_cut_corners() {
        let cloud = rectangle(10.0);
        assert_eq!(cloud.vertices.len(), 32);
        assert!(cloud.vertices.iter().all(|vertex| (vertex.bulge + arc_bulge()).abs() < 1e-12));
        assert_eq!(arc_length_of(&cloud), Some(10.0));
        let Some(Guide::Polygon(corners)) = guide(&cloud) else {
            panic!("a polygon guide");
        };
        let want = [[0.0, 0.0], [0.0, 60.0], [100.0, 60.0], [100.0, 0.0]];
        assert_eq!(corners.len(), want.len());
        for (got, want) in corners.iter().zip(want) {
            assert!((got[0] - want[0]).abs() < 1e-9 && (got[1] - want[1]).abs() < 1e-9, "{corners:?}");
        }
    }
}
