// Point cloud extraction: PCEXTRACTEDGE, PCEXTRACTCORNER,
// PCEXTRACTCENTERLINE and PCEXTRACTSECTION (and -PCEXTRACTSECTION).
//
//   PCEXTRACTEDGE    Specify the first plane: / Specify the second plane:
//                    → a line where the two planes meet, over the points'
//                      extent along it.
//   PCEXTRACTCORNER  … / Specify the third plane: → a point where the
//                    three planes meet.
//   PCEXTRACTCENTERLINE  Select a cylindrical segment: → a line along the
//                    cylinder's axis, over its extent.
//   PCEXTRACTSECTION Select point cloud: → polylines (or lines) tracing the
//                    points on the kept side of a live section plane, seen
//                    on the plane; then
//                    Do you want to accept the results or change settings ?
//                    [Accept/Settings/Undo] <Settings>:
//                    (-PCEXTRACTSECTION creates them without asking.)
//
// Planes, cylinders and section lines come from structured (segmented)
// scans only; on an unstructured scan a plane or cylinder pick asks again
// and a section extracts nothing, as the reference does. On a
// structured scan a plane is found from the cloud points under the cursor:
// the front-most point in the pick aperture, a plane fitted to the points
// around it, then grown over the points on that plane connected to it.

use std::sync::{Arc, Mutex};

use codec::entities::{LwPolyline, LwVertex};
use codec::types::{Color, Handle, Vector2, Vector3};
use codec::{EntityType, Line, Point as CadPoint};
use glam::{DMat3, DVec2, DVec3};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext, WorkingPlane};
use crate::scene::model::point_cloud::PlacedCloud;
use crate::scene::model::wire_model::WireModel;

const INVALID_POINT: &str = "Invalid point.";
const SAME_AS_FIRST: &str = "The plane selected is the same as the first one.";
const SAME_AS_SECOND: &str = "The plane selected is the same as the second one.";
const NEAR_PARALLEL: &str = "Planes are near parallel, no edge or corner extracted.";

/// The shown points of every point cloud in the drawing.
#[derive(Clone, Default)]
pub struct Clouds(pub Vec<(Handle, Arc<PlacedCloud>)>);

impl Clouds {
    fn points(&self) -> impl Iterator<Item = DVec3> + '_ {
        self.0.iter().flat_map(|(_, cloud)| {
            cloud.instances.iter().map(|p| {
                DVec3::new(
                    p.pos[0] as f64 + p.pos_low[0] as f64,
                    p.pos[1] as f64 + p.pos_low[1] as f64,
                    p.pos[2] as f64 + p.pos_low[2] as f64,
                )
            })
        })
    }

    /// Whether the clouds carry segments (structured scans) to extract from.
    // ponytail: scan segmentation is not decoded, so every scan counts as
    // unstructured; read the scan's segmentation to extract from structured ones.
    fn segmented(&self) -> bool {
        false
    }

    fn of(&self, handle: Handle) -> Option<Clouds> {
        let found: Vec<_> = self.0.iter().filter(|(h, _)| *h == handle).cloned().collect();
        (!found.is_empty()).then_some(Clouds(found))
    }
}

// ── Fitting ───────────────────────────────────────────────────────────────

/// Eigenvalues (ascending) and unit eigenvectors of a symmetric 3×3 matrix
/// (Jacobi rotations).
fn eigen(m: DMat3) -> ([f64; 3], [DVec3; 3]) {
    let mut a = m.to_cols_array_2d();
    let mut v = DMat3::IDENTITY.to_cols_array_2d();
    for _ in 0..32 {
        let (mut p, mut q, mut off) = (0, 1, 0.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            if a[i][j].abs() > off {
                (p, q, off) = (i, j, a[i][j].abs());
            }
        }
        if off < 1e-18 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = (if theta >= 0.0 { 1.0 } else { -1.0 }) / (theta.abs() + (theta * theta + 1.0).sqrt());
        let (c, s) = (1.0 / (t * t + 1.0).sqrt(), t / (t * t + 1.0).sqrt());
        for k in 0..3 {
            let (akp, akq) = (a[k][p], a[k][q]);
            a[k][p] = c * akp - s * akq;
            a[k][q] = s * akp + c * akq;
        }
        for k in 0..3 {
            let (apk, aqk) = (a[p][k], a[q][k]);
            a[p][k] = c * apk - s * aqk;
            a[q][k] = s * apk + c * aqk;
        }
        for row in &mut v {
            let (vp, vq) = (row[p], row[q]);
            row[p] = c * vp - s * vq;
            row[q] = s * vp + c * vq;
        }
    }
    // `v[row][col]`: column j is eigenvector j, the loaded matrix's row j.
    let vm = DMat3::from_cols_array_2d(&v);
    let mut order = [0, 1, 2];
    order.sort_by(|&x, &y| a[x][x].total_cmp(&a[y][y]));
    (order.map(|k| a[k][k]), order.map(|k| vm.row(k).normalize_or_zero()))
}

/// Centroid and covariance of `points`.
fn moments(points: &[DVec3]) -> (DVec3, DMat3) {
    let n = points.len().max(1) as f64;
    let c = points.iter().copied().sum::<DVec3>() / n;
    let mut m = DMat3::ZERO;
    for p in points {
        let d = *p - c;
        m += DMat3::from_cols(d * d.x, d * d.y, d * d.z);
    }
    (c, m * (1.0 / n))
}

/// Every n-th point, at most `max` of them.
fn thin(points: Vec<DVec3>, max: usize) -> Vec<DVec3> {
    let step = points.len().div_ceil(max.max(1)).max(1);
    points.into_iter().step_by(step).collect()
}

/// A planar region of the cloud.
#[derive(Clone)]
struct Plane {
    center: DVec3,
    normal: DVec3,
    /// Distance from the plane still counted on it.
    tolerance: f64,
    /// The region's points (thinned).
    region: Vec<DVec3>,
}

impl Plane {
    fn same_as(&self, other: &Plane) -> bool {
        self.normal.dot(other.normal).abs() > 5f64.to_radians().cos()
            && self.normal.dot(other.center - self.center).abs() < 2.0 * self.tolerance.max(other.tolerance)
    }
}

/// Where a pick lands on the cloud: the front-most point in the aperture,
/// and the drawing size of one pixel there.
fn surface_hit(clouds: &Clouds, context: &PointPickContext, pt: DVec3) -> Option<(DVec3, f64)> {
    let screen = |p: DVec3| crate::scene::pick::hit_test::world_to_screen(p, context.view, context.eye, context.bounds);
    let cursor = screen(pt);
    let radius = context.aperture_px.max(4.0);
    let hit = clouds
        .points()
        .filter(|p| {
            let s = screen(*p);
            (s.x - cursor.x).hypot(s.y - cursor.y) <= radius
        })
        .min_by(|a, b| a.distance_squared(context.eye).total_cmp(&b.distance_squared(context.eye)))?;
    let right = context.view.row(0).truncate().as_dvec3().normalize_or(DVec3::X);
    let (a, b) = (screen(hit), screen(hit + right));
    let px_per_unit = (b.x - a.x).hypot(b.y - a.y) as f64;
    (px_per_unit > 0.0).then(|| (hit, 1.0 / px_per_unit))
}

/// The points within `radius` of `at`.
fn around(clouds: &Clouds, at: DVec3, radius: f64) -> Vec<DVec3> {
    clouds.points().filter(|p| p.distance_squared(at) <= radius * radius).collect()
}

/// The planar region under a pick, or `None` where the points there do
/// not lie on a plane.
fn detect_plane(clouds: &Clouds, hit: DVec3, pixel: f64) -> Option<Plane> {
    let mut radius = 12.0 * pixel;
    let mut near = around(clouds, hit, radius);
    while near.len() < 30 && radius < 200.0 * pixel {
        radius *= 2.0;
        near = around(clouds, hit, radius);
    }
    if near.len() < 10 {
        return None;
    }
    let (center, m) = moments(&thin(near, 20_000));
    let (values, vectors) = eigen(m);
    // Flat: the spread off the plane is small beside the spread along it.
    if values[0] > 0.05 * values[1] {
        return None;
    }
    let mut plane = Plane {
        center,
        normal: vectors[0],
        tolerance: (3.0 * values[0].max(0.0).sqrt()).max(0.5 * pixel),
        region: Vec::new(),
    };
    let cell = radius.max(2.0 * plane.tolerance);
    for _ in 0..2 {
        plane.region = connected_region(clouds, &plane, hit, cell);
        if plane.region.len() < 10 {
            return None;
        }
        let (c, m) = moments(&plane.region);
        let (values, vectors) = eigen(m);
        plane.center = c;
        plane.normal = vectors[0];
        plane.tolerance = (3.0 * values[0].max(0.0).sqrt()).max(0.5 * pixel).min(plane.tolerance * 2.0);
    }
    Some(plane)
}

type Cell = (i64, i64, i64);

fn cell_of(p: DVec3, size: f64) -> Cell {
    ((p.x / size).floor() as i64, (p.y / size).floor() as i64, (p.z / size).floor() as i64)
}

/// Grid cells reached from `start` through occupied neighbours (26-way).
fn flood(occupied: &FxHashSet<Cell>, start: Cell) -> FxHashSet<Cell> {
    let mut reached = FxHashSet::default();
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        if !occupied.contains(&c) || !reached.insert(c) {
            continue;
        }
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    stack.push((c.0 + dx, c.1 + dy, c.2 + dz));
                }
            }
        }
    }
    reached
}

/// The cloud points on `plane` connected to `seed` (thinned).
fn connected_region(clouds: &Clouds, plane: &Plane, seed: DVec3, cell: f64) -> Vec<DVec3> {
    let on = |p: &DVec3| plane.normal.dot(*p - plane.center).abs() <= plane.tolerance;
    let occupied: FxHashSet<Cell> = clouds.points().filter(on).map(|p| cell_of(p, cell)).collect();
    let reached = flood(&occupied, cell_of(seed, cell));
    let region: Vec<DVec3> = clouds.points().filter(|p| on(p) && reached.contains(&cell_of(*p, cell))).collect();
    thin(region, 50_000)
}

/// The cylinder under a pick: its axis (a point and direction), and the
/// line along it over the cylinder's extent.
fn detect_cylinder(clouds: &Clouds, hit: DVec3, pixel: f64) -> Option<(DVec3, DVec3)> {
    let radius = 30.0 * pixel;
    let near = thin(around(clouds, hit, radius), 20_000);
    if near.len() < 30 {
        return None;
    }
    // Normals of small patches; the axis is the direction they all miss.
    let cell = radius / 5.0;
    let mut patches: FxHashMap<Cell, Vec<DVec3>> = FxHashMap::default();
    for p in &near {
        patches.entry(cell_of(*p, cell)).or_default().push(*p);
    }
    let mut spread = DMat3::ZERO;
    for points in patches.values().filter(|points| points.len() >= 6) {
        let (_, m) = moments(points);
        let (values, vectors) = eigen(m);
        if values[0] < 0.2 * values[1] {
            let n = vectors[0];
            spread += DMat3::from_cols(n * n.x, n * n.y, n * n.z);
        }
    }
    let (values, vectors) = eigen(spread);
    // A plane's normals all agree; a cylinder's turn about its axis.
    if values[2] <= 0.0 || values[1] < 0.05 * values[2] {
        return None;
    }
    let axis = vectors[0];
    let (e1, e2) = (vectors[1], vectors[2]);
    let flat: Vec<DVec2> = near.iter().map(|p| DVec2::new(e1.dot(*p - hit), e2.dot(*p - hit))).collect();
    let (centre, r, rms) = fit_circle(&flat)?;
    if rms > 0.25 * r || r > 10.0 * radius {
        return None;
    }
    let origin = hit + e1 * centre.x + e2 * centre.y;
    // Grow along the axis over the points on the surface, in steps of `cell`.
    let tolerance = (3.0 * rms).max(0.02 * r).max(0.5 * pixel);
    let slices: FxHashSet<i64> = clouds
        .points()
        .filter(|p| {
            let d = *p - origin;
            ((d - axis * d.dot(axis)).length() - r).abs() <= tolerance
        })
        .map(|p| (axis.dot(p - origin) / cell).floor() as i64)
        .collect();
    let start = (axis.dot(hit - origin) / cell).floor() as i64;
    let reach = |step: i64| {
        let mut k = start;
        // Gaps up to two slices are bridged.
        while (1..=3).any(|g| slices.contains(&(k + step * g))) {
            k += step * (1..=3).find(|g| slices.contains(&(k + step * g))).unwrap_or(1);
        }
        k
    };
    let (low, high) = (reach(-1), reach(1));
    Some((origin + axis * (low as f64 * cell), origin + axis * ((high + 1) as f64 * cell)))
}

/// Algebraic circle fit: centre, radius and RMS residual.
fn fit_circle(points: &[DVec2]) -> Option<(DVec2, f64, f64)> {
    // x² + y² + D x + E y + F = 0 in least squares.
    let mut m = DMat3::ZERO;
    let mut b = DVec3::ZERO;
    for p in points {
        let row = DVec3::new(p.x, p.y, 1.0);
        m += DMat3::from_cols(row * row.x, row * row.y, row * row.z);
        b += row * -(p.x * p.x + p.y * p.y);
    }
    if m.determinant().abs() < 1e-30 {
        return None;
    }
    let s = m.inverse() * b;
    let centre = DVec2::new(-s.x / 2.0, -s.y / 2.0);
    let r2 = centre.length_squared() - s.z;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let rms = (points.iter().map(|p| (p.distance(centre) - r).powi(2)).sum::<f64>() / points.len() as f64).sqrt();
    Some((centre, r, rms))
}

fn vector(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}

// ── PCEXTRACTEDGE / PCEXTRACTCORNER ────────────────────────────────────────

pub struct PlanesCommand {
    corner: bool,
    clouds: Clouds,
    context: Option<PointPickContext>,
    planes: Vec<Plane>,
}

impl PlanesCommand {
    pub fn new(corner: bool, clouds: Clouds) -> Self {
        Self { corner, clouds, context: None, planes: Vec::new() }
    }

    fn edge(&self) -> CmdResult {
        let (a, b) = (&self.planes[0], &self.planes[1]);
        let direction = a.normal.cross(b.normal).normalize();
        let rows = DMat3::from_cols(a.normal, b.normal, direction).transpose();
        let origin = rows.inverse()
            * DVec3::new(a.normal.dot(a.center), b.normal.dot(b.center), direction.dot((a.center + b.center) * 0.5));
        // Over the points of either plane next to the line; else where
        // both planes' points overlap along it.
        let width = 3.0 * a.tolerance.max(b.tolerance);
        let along = |p: &DVec3| direction.dot(*p - origin);
        let near: Vec<f64> = a
            .region
            .iter()
            .chain(&b.region)
            .filter(|p| {
                let d = **p - origin;
                (d - direction * d.dot(direction)).length() <= width
            })
            .map(along)
            .collect();
        let span = |points: &[DVec3]| {
            points.iter().map(along).fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)))
        };
        let (low, high) = if near.len() >= 2 {
            near.iter().fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(*t), hi.max(*t)))
        } else {
            let ((a0, a1), (b0, b1)) = (span(&a.region), span(&b.region));
            if a0.max(b0) < a1.min(b1) { (a0.max(b0), a1.min(b1)) } else { (a0.min(b0), a1.max(b1)) }
        };
        let line = Line::from_points(vector(origin + direction * low), vector(origin + direction * high));
        CmdResult::CommitAndExit(EntityType::Line(line))
    }

    fn corner(&self) -> CmdResult {
        let [a, b, c] = [&self.planes[0], &self.planes[1], &self.planes[2]];
        let rows = DMat3::from_cols(a.normal, b.normal, c.normal).transpose();
        if rows.determinant().abs() < 5f64.to_radians().sin() {
            return CmdResult::CancelWithMessage(NEAR_PARALLEL.to_string());
        }
        let at = rows.inverse() * DVec3::new(a.normal.dot(a.center), b.normal.dot(b.center), c.normal.dot(c.center));
        CmdResult::CommitAndExit(EntityType::Point(CadPoint { location: vector(at), ..Default::default() }))
    }
}

impl CadCommand for PlanesCommand {
    fn name(&self) -> &'static str {
        if self.corner { "PCEXTRACTCORNER" } else { "PCEXTRACTEDGE" }
    }

    fn prompt(&self) -> String {
        match self.planes.len() {
            0 => "Specify the first plane:",
            1 => "Specify the second plane:",
            _ => "Specify the third plane:",
        }
        .to_string()
    }

    fn wants_point_pick_context(&self) -> bool {
        true
    }

    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(context) = self.context else {
            return CmdResult::NeedPoint;
        };
        // Off the cloud, not on a plane or an unstructured scan: the prompt again.
        if !self.clouds.segmented() {
            return CmdResult::NeedPoint;
        }
        let Some(plane) = surface_hit(&self.clouds, &context, pt)
            .and_then(|(hit, pixel)| detect_plane(&self.clouds, hit, pixel))
        else {
            return CmdResult::NeedPoint;
        };
        if let Some(first) = self.planes.first() {
            if plane.same_as(first) {
                return CmdResult::ReportError(SAME_AS_FIRST.to_string());
            }
            if self.planes.get(1).is_some_and(|second| plane.same_as(second)) {
                return CmdResult::ReportError(SAME_AS_SECOND.to_string());
            }
            if self.planes.iter().any(|p| p.normal.dot(plane.normal).abs() > 5f64.to_radians().cos()) {
                return CmdResult::CancelWithMessage(NEAR_PARALLEL.to_string());
            }
        }
        self.planes.push(plane);
        match (self.corner, self.planes.len()) {
            (false, 2) => self.edge(),
            (true, 3) => self.corner(),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::ReportError(INVALID_POINT.to_string())
    }
}

// ── PCEXTRACTCENTERLINE ────────────────────────────────────────────────────

pub struct CenterlineCommand {
    clouds: Clouds,
    context: Option<PointPickContext>,
}

impl CenterlineCommand {
    pub fn new(clouds: Clouds) -> Self {
        Self { clouds, context: None }
    }
}

impl CadCommand for CenterlineCommand {
    fn name(&self) -> &'static str {
        "PCEXTRACTCENTERLINE"
    }

    fn prompt(&self) -> String {
        "Select a cylindrical segment:".to_string()
    }

    fn wants_point_pick_context(&self) -> bool {
        true
    }

    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(context) = self.context else {
            return CmdResult::NeedPoint;
        };
        if !self.clouds.segmented() {
            return CmdResult::NeedPoint;
        }
        match surface_hit(&self.clouds, &context, pt).and_then(|(hit, pixel)| detect_cylinder(&self.clouds, hit, pixel)) {
            Some((start, end)) => CmdResult::CommitAndExit(EntityType::Line(Line::from_points(vector(start), vector(end)))),
            None => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::ReportError(INVALID_POINT.to_string())
    }
}

// ── PCEXTRACTSECTION ───────────────────────────────────────────────────────

/// A section object: a point on its plane, the direction it looks in (the
/// kept side) and the direction along its line. `live` is its live-section
/// state.
#[derive(Clone, Copy)]
pub struct Section {
    pub origin: DVec3,
    pub viewing: DVec3,
    pub tangent: DVec3,
    pub live: bool,
}

/// The extraction settings (the reference's dialog defaults), kept for the
/// session.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub perimeter: bool,
    pub max_points: usize,
    /// The layer the lines go on; `None` is the current layer.
    pub layer: Option<String>,
    /// The lines' colour (ACI; 256 ByLayer, 0 ByBlock).
    pub color: i16,
    pub polylines: bool,
    pub width: f64,
    pub min_length: f64,
    pub connect: f64,
    pub angle: f64,
    /// Show the result and ask to accept it (else create it at once).
    pub preview: bool,
}

const DEFAULTS: Settings = Settings {
    perimeter: false,
    max_points: 18_000,
    layer: None,
    color: 3,
    polylines: true,
    width: 0.0,
    min_length: 150.0,
    connect: 300.0,
    angle: 5.0,
    preview: true,
};

static SETTINGS: Mutex<Settings> = Mutex::new(DEFAULTS);

pub fn settings() -> Settings {
    SETTINGS.lock().map(|s| s.clone()).unwrap_or(DEFAULTS)
}

pub fn set_settings(settings: Settings) {
    if let Ok(mut stored) = SETTINGS.lock() {
        *stored = settings;
    }
}

/// The cloud and section a PCEXTRACTSECTION is extracting from while its
/// settings dialog is open.
static JOB: Mutex<Option<(Clouds, Section)>> = Mutex::new(None);

/// Whether an extraction waits for its settings dialog.
pub fn has_job() -> bool {
    JOB.lock().is_ok_and(|job| job.is_some())
}

pub fn drop_job() {
    if let Ok(mut job) = JOB.lock() {
        *job = None;
    }
}

const TOO_FEW: &str = "Too few points to process. No geometry extracted from point cloud.";
const ACCEPT: &str = "Do you want to accept the results or change settings ? [Accept/Settings/Undo] <Settings>:";

/// Chains (plane coordinates) tracing the points seen on the section.
fn trace(points: &[DVec2], s: &Settings) -> Vec<Vec<DVec2>> {
    let (lo, hi) = points.iter().fold((DVec2::MAX, DVec2::MIN), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    // ponytail: grid resolution tied to the minimum length, capped at 1000
    // cells a side; a finer trace needs a proper curve fit.
    let cell = s.min_length.max((hi - lo).max_element() / 1000.0).max(1e-9);
    let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut cells: FxHashMap<(i64, i64), (DVec2, usize)> = FxHashMap::default();
    for p in points {
        let entry = cells.entry(key(*p)).or_insert((DVec2::ZERO, 0));
        entry.0 += *p;
        entry.1 += 1;
    }
    // The area the points cover: their cells, and gaps mostly surrounded
    // by them. Only its edges are traced — every edge, or for Perimeter
    // only the outer one.
    let ring = |(x, y): (i64, i64)| {
        [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)].map(|(dx, dy)| (x + dx, y + dy))
    };
    let mut filled: FxHashSet<(i64, i64)> = cells.keys().copied().collect();
    let gaps: Vec<(i64, i64)> = cells
        .keys()
        .flat_map(|k| ring(*k))
        .filter(|c| !cells.contains_key(c) && ring(*c).iter().filter(|n| cells.contains_key(n)).count() >= 5)
        .collect();
    filled.extend(gaps);
    let outside = s.perimeter.then(|| {
        let (x0, x1) = cells.keys().fold((i64::MAX, i64::MIN), |(a, b), k| (a.min(k.0 - 1), b.max(k.0 + 1)));
        let (y0, y1) = cells.keys().fold((i64::MAX, i64::MIN), |(a, b), k| (a.min(k.1 - 1), b.max(k.1 + 1)));
        let mut reached = FxHashSet::default();
        let mut stack = vec![(x0, y0)];
        while let Some(c) = stack.pop() {
            if c.0 < x0 || c.0 > x1 || c.1 < y0 || c.1 > y1 || filled.contains(&c) || !reached.insert(c) {
                continue;
            }
            stack.extend([(c.0 + 1, c.1), (c.0 - 1, c.1), (c.0, c.1 + 1), (c.0, c.1 - 1)]);
        }
        reached
    });
    let mut keys: Vec<(i64, i64)> = cells
        .keys()
        .copied()
        .filter(|&(x, y)| {
            [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]
                .iter()
                .any(|c| !filled.contains(c) && outside.as_ref().is_none_or(|o| o.contains(c)))
        })
        .collect();
    keys.sort_unstable();
    let index: FxHashMap<(i64, i64), usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let nodes: Vec<DVec2> = keys.iter().map(|k| cells[k].0 / cells[k].1 as f64).collect();
    // Links within the connect tolerance, shortest first, each node joining
    // at most two and none closing a loop: chains.
    // ponytail: links reach ten cells at most, whatever the tolerance.
    let reach = (s.connect / cell).ceil().clamp(1.0, 10.0) as i64;
    let mut edges = Vec::new();
    for (i, &(x, y)) in keys.iter().enumerate() {
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                if let Some(&j) = index.get(&(x + dx, y + dy)) {
                    let d = nodes[i].distance(nodes[j]);
                    if j > i && d <= s.connect {
                        edges.push((d, i, j));
                    }
                }
            }
        }
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut root: Vec<usize> = (0..nodes.len()).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    let mut links: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (_, i, j) in edges {
        if links[i].len() < 2 && links[j].len() < 2 {
            let (ri, rj) = (find(&mut root, i), find(&mut root, j));
            if ri != rj {
                root[ri] = rj;
                links[i].push(j);
                links[j].push(i);
            }
        }
    }
    let mut seen = vec![false; nodes.len()];
    let mut chains = Vec::new();
    for start in 0..nodes.len() {
        if seen[start] || links[start].len() != 1 {
            continue;
        }
        let (mut chain, mut previous, mut at) = (Vec::new(), usize::MAX, start);
        loop {
            seen[at] = true;
            chain.push(nodes[at]);
            match links[at].iter().find(|&&n| n != previous && !seen[n]) {
                Some(&next) => (previous, at) = (at, next),
                None => break,
            }
        }
        let chain = simplify(chain, s);
        if chain.len() >= 2 {
            chains.push(chain);
        }
    }
    chains
}

/// Straightens a chain: drops vertices off-line by under half a cell, then
/// those turning less than the collinear angle, then merges segments
/// shorter than the minimum length. Nothing under the minimum length is
/// kept.
fn simplify(chain: Vec<DVec2>, s: &Settings) -> Vec<DVec2> {
    fn douglas(points: &[DVec2], tolerance: f64, out: &mut Vec<DVec2>) {
        let (a, b) = (points[0], points[points.len() - 1]);
        let dir = (b - a).normalize_or_zero();
        let far = (1..points.len() - 1)
            .map(|i| (i, (points[i] - a).perp_dot(dir).abs()))
            .max_by(|x, y| x.1.total_cmp(&y.1));
        match far {
            Some((i, d)) if d > tolerance => {
                douglas(&points[..=i], tolerance, out);
                out.pop();
                douglas(&points[i..], tolerance, out);
            }
            _ => out.extend([a, b]),
        }
    }
    let mut out = Vec::new();
    douglas(&chain, 0.5 * s.min_length, &mut out);
    let turn = s.angle.to_radians();
    let mut k = 1;
    while k + 1 < out.len() {
        let (u, v) = (out[k] - out[k - 1], out[k + 1] - out[k]);
        if u.angle_to(v).abs() < turn || u.length() < s.min_length || v.length() < s.min_length {
            out.remove(k);
        } else {
            k += 1;
        }
    }
    let length: f64 = out.windows(2).map(|w| w[0].distance(w[1])).sum();
    if length < s.min_length { Vec::new() } else { out }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionStep {
    Select,
    Accept,
    Extract,
    MinLength,
    Connect,
    Angle,
    MaxPoints,
    Output,
}

pub struct SectionCommand {
    /// PCEXTRACTSECTION previews and asks; -PCEXTRACTSECTION does not.
    ask: bool,
    clouds: Clouds,
    sections: Vec<Section>,
    step: SectionStep,
    picked: bool,
    cloud: Option<Clouds>,
    section: Option<Section>,
    result: Vec<EntityType>,
}

impl SectionCommand {
    pub fn new(ask: bool, clouds: Clouds, sections: Vec<Section>) -> Self {
        Self {
            ask,
            clouds,
            sections,
            step: SectionStep::Select,
            picked: false,
            cloud: None,
            section: None,
            result: Vec::new(),
        }
    }

    /// The entities the settings give for the chosen cloud and section.
    fn extract(&self) -> Result<Vec<EntityType>, &'static str> {
        let (Some(cloud), Some(section)) = (&self.cloud, self.section) else {
            return Ok(Vec::new());
        };
        // An unstructured scan gives no section lines (nothing, silently).
        if !cloud.segmented() {
            return Ok(Vec::new());
        }
        let s = settings();
        let n = section.viewing;
        let kept: Vec<DVec3> = cloud
            .points()
            // ponytail: every section kind cuts as a plane; slice depth and
            // boundaries are not applied.
            .filter(|p| n.dot(*p - section.origin) >= 0.0)
            .collect();
        if kept.len() < 3 {
            return Err(TOO_FEW);
        }
        let plane = WorkingPlane::new(section.origin, section.tangent, n.cross(section.tangent));
        // The plane's own frame has the section's normal as Z.
        let plane = if plane.z.dot(n) < 0.0 { WorkingPlane::new(section.origin, -section.tangent, n.cross(-section.tangent)) } else { plane };
        let flat: Vec<DVec2> = thin(kept, s.max_points)
            .into_iter()
            .map(|p| plane.to_local(p).truncate())
            .collect();
        let color = match s.color {
            256 => Color::ByLayer,
            0 => Color::ByBlock,
            index => Color::from_index(index),
        };
        let mut out = Vec::new();
        for chain in trace(&flat, &s) {
            if s.polylines {
                let mut pline = LwPolyline {
                    vertices: chain.iter().map(|p| LwVertex::new(Vector2::new(p.x, p.y))).collect(),
                    constant_width: s.width,
                    ..Default::default()
                };
                pline.common.color = color.clone();
                if let Some(layer) = &s.layer {
                    pline.common.layer = layer.clone();
                }
                out.push(plane.place_entity(EntityType::LwPolyline(pline)));
            } else {
                for w in chain.windows(2) {
                    let mut line = Line::from_points(
                        vector(plane.to_world(w[0].extend(0.0))),
                        vector(plane.to_world(w[1].extend(0.0))),
                    );
                    line.common.color = color.clone();
                    if let Some(layer) = &s.layer {
                        line.common.layer = layer.clone();
                    }
                    out.push(EntityType::Line(line));
                }
            }
        }
        Ok(out)
    }

    /// PCEXTRACTSECTION after its settings dialog: the waiting cloud and
    /// section, extracted with the settings.
    pub fn resume() -> Option<Self> {
        let (cloud, section) = JOB.lock().ok()?.take()?;
        let mut command = Self::new(true, Clouds::default(), Vec::new());
        command.cloud = Some(cloud);
        command.section = Some(section);
        command.step = SectionStep::Accept;
        Some(command)
    }

    /// Extracts; asks to accept when previewing, else creates.
    pub fn run(&mut self) -> CmdResult {
        match self.extract() {
            Err(message) => CmdResult::CancelWithMessage(message.to_string()),
            Ok(entities) if !self.ask || !settings().preview => {
                if entities.is_empty() { CmdResult::Cancel } else { CmdResult::CommitEntitiesAndExit(entities) }
            }
            Ok(entities) => {
                self.result = entities;
                self.step = SectionStep::Accept;
                CmdResult::InterimWire(self.preview())
            }
        }
    }

    fn preview(&self) -> WireModel {
        let mut points = Vec::new();
        for entity in &self.result {
            match entity {
                EntityType::LwPolyline(p) => {
                    let plane = ocs(p.normal);
                    points.extend(p.vertices.iter().map(|v| {
                        (plane * DVec3::new(v.location.x, v.location.y, p.elevation)).to_array()
                    }));
                }
                EntityType::Line(l) => points.extend([l.start, l.end].map(|v| [v.x, v.y, v.z])),
                _ => continue,
            }
            points.push([f64::NAN; 3]);
        }
        WireModel::solid_f64("pc_extract_section".into(), points, [0.0, 1.0, 0.0, 1.0], false)
    }

    fn setting_prompt(&self) -> String {
        let s = settings();
        match self.step {
            SectionStep::Extract => format!(
                "Extract [Entire cross section/Perimeter only] <{}>:",
                if s.perimeter { "Perimeter only" } else { "Entire cross section" }
            ),
            SectionStep::MinLength => format!("Minimum line length <{}>:", s.min_length),
            SectionStep::Connect => format!("Connect lines tolerance <{}>:", s.connect),
            SectionStep::Angle => format!("Collinear angle tolerance <{}>:", s.angle),
            SectionStep::MaxPoints => format!("Maximum points to process <{}>:", s.max_points),
            SectionStep::Output => format!(
                "Output geometry [Lines/2D Polylines] <{}>:",
                if s.polylines { "2D Polylines" } else { "Lines" }
            ),
            _ => String::new(),
        }
    }

    /// One settings answer; an empty one keeps the value.
    fn set(&mut self, text: &str) -> CmdResult {
        let text = text.trim();
        let number = crate::entities::common::parse_f64(text);
        let mut s = settings();
        let next = match self.step {
            SectionStep::Extract => {
                match text.to_ascii_uppercase().as_str() {
                    "" => {}
                    "E" | "ENTIRE" => s.perimeter = false,
                    "P" | "PERIMETER" => s.perimeter = true,
                    _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
                }
                SectionStep::MinLength
            }
            SectionStep::MinLength | SectionStep::Connect if !text.is_empty() => match number.filter(|v| *v > 0.0) {
                Some(v) if self.step == SectionStep::MinLength => {
                    s.min_length = v;
                    SectionStep::Connect
                }
                Some(v) => {
                    s.connect = v;
                    SectionStep::Angle
                }
                None => return CmdResult::ReportError("Requires a positive number.".to_string()),
            },
            SectionStep::MinLength => SectionStep::Connect,
            SectionStep::Connect => SectionStep::Angle,
            SectionStep::Angle if !text.is_empty() => match text.parse::<i64>() {
                Ok(v) if (0..=10).contains(&v) => {
                    s.angle = v as f64;
                    SectionStep::MaxPoints
                }
                _ => return CmdResult::ReportError("Colinear angle tolerance only accepts integer from 0 to 10.".to_string()),
            },
            SectionStep::Angle => SectionStep::MaxPoints,
            SectionStep::MaxPoints if !text.is_empty() => match text.parse::<usize>() {
                Ok(v) if v > 0 => {
                    s.max_points = v;
                    SectionStep::Output
                }
                _ => return CmdResult::ReportError("Requires a positive integer.".to_string()),
            },
            SectionStep::MaxPoints => SectionStep::Output,
            SectionStep::Output => {
                match text.to_ascii_uppercase().as_str() {
                    "" => {}
                    "L" | "LINES" => s.polylines = false,
                    "2" | "2D" | "P" | "POLYLINES" | "2D POLYLINES" => s.polylines = true,
                    _ => return CmdResult::ReportError("Invalid option keyword.".to_string()),
                }
                set_settings(s);
                return self.run();
            }
            _ => return CmdResult::NeedPoint,
        };
        set_settings(s);
        self.step = next;
        CmdResult::NeedPoint
    }
}

/// The dialog's pick buttons: a distance measured by two points on screen,
/// then the dialog again.
pub struct SectionDistanceCommand {
    /// Connect lines tolerance (else minimum line length).
    connect: bool,
    first: Option<DVec3>,
}

impl SectionDistanceCommand {
    pub fn new(connect: bool) -> Self {
        Self { connect, first: None }
    }
}

impl CadCommand for SectionDistanceCommand {
    fn name(&self) -> &'static str {
        "PCEXTRACTSECTION"
    }

    fn prompt(&self) -> String {
        if self.first.is_none() { "Specify first point:" } else { "Specify second point:" }.to_string()
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(first) = self.first else {
            self.first = Some(pt);
            return CmdResult::NeedPoint;
        };
        let mut s = settings();
        let distance = first.distance(pt);
        if distance > 0.0 {
            if self.connect {
                s.connect = distance;
            } else {
                s.min_length = distance;
            }
            set_settings(s);
        }
        CmdResult::Dispatch("_PCSECTIONDLG".to_string())
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch("_PCSECTIONDLG".to_string())
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let first = self.first?;
        Some(WireModel::solid_f64("pc_section_distance".into(), vec![first.to_array(), pt.to_array()], [1.0, 1.0, 1.0, 1.0], false))
    }
}

/// The OCS axes of an entity normal (arbitrary axis algorithm) as columns.
fn ocs(normal: Vector3) -> DMat3 {
    let n = DVec3::new(normal.x, normal.y, normal.z).normalize_or(DVec3::Z);
    let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { DVec3::Y.cross(n) } else { DVec3::Z.cross(n) }.normalize();
    DMat3::from_cols(ax, n.cross(ax), n)
}

impl CadCommand for SectionCommand {
    fn name(&self) -> &'static str {
        if self.ask { "PCEXTRACTSECTION" } else { "-PCEXTRACTSECTION" }
    }

    fn prompt(&self) -> String {
        match self.step {
            SectionStep::Select => "Select point cloud:".to_string(),
            SectionStep::Accept => ACCEPT.to_string(),
            _ => self.setting_prompt(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            SectionStep::Accept => vec![
                CmdOption::new("Accept", "A"),
                CmdOption::new("Settings", "S"),
                CmdOption::new("Undo", "U"),
            ],
            SectionStep::Extract => vec![CmdOption::new("Entire cross section", "E"), CmdOption::new("Perimeter only", "P")],
            SectionStep::Output => vec![CmdOption::new("Lines", "L"), CmdOption::new("2D Polylines", "2D")],
            _ => Vec::new(),
        }
    }

    fn preserve_commit_style(&self) -> bool {
        true
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            SectionStep::Select => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == SectionStep::Select
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = matches!(
            &entity,
            EntityType::Extended(e) if matches!(e.data, codec::entities::ExtendedEntityData::PointCloudEx(_))
        );
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !std::mem::take(&mut self.picked) {
            return CmdResult::NeedPoint;
        }
        // A live section plane is needed; the first one found is used.
        if self.sections.is_empty() {
            return CmdResult::CancelWithMessage("Section object not found.".to_string());
        }
        let Some(section) = self.sections.iter().find(|s| s.live).copied() else {
            return CmdResult::CancelWithMessage("Live section must be turned on for section object.".to_string());
        };
        let Some(cloud) = self.clouds.of(handle) else {
            return CmdResult::CancelWithMessage("The point cloud is not loaded.".to_string());
        };
        // PCEXTRACTSECTION asks its settings in the dialog first.
        if self.ask {
            if let Ok(mut job) = JOB.lock() {
                *job = Some((cloud, section));
            }
            return CmdResult::Dispatch("_PCSECTIONDLG".to_string());
        }
        self.cloud = Some(cloud);
        self.section = Some(section);
        self.run()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            SectionStep::Select => return None,
            SectionStep::Accept => match text.trim().to_ascii_uppercase().as_str() {
                "A" | "ACCEPT" => {
                    let entities = std::mem::take(&mut self.result);
                    if entities.is_empty() { CmdResult::Cancel } else { CmdResult::CommitEntitiesAndExit(entities) }
                }
                // Back to the settings dialog with the same cloud and section.
                "" | "S" | "SETTINGS" if self.ask => {
                    if let (Some(cloud), Some(section), Ok(mut job)) = (self.cloud.take(), self.section, JOB.lock()) {
                        *job = Some((cloud, section));
                    }
                    CmdResult::Dispatch("_PCSECTIONDLG".to_string())
                }
                "" | "S" | "SETTINGS" => {
                    self.step = SectionStep::Extract;
                    CmdResult::NeedPoint
                }
                "U" | "UNDO" => CmdResult::Cancel,
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            _ => self.set(text),
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            SectionStep::Select => CmdResult::Cancel,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }

    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["PCEXTRACTEDGE", "PCEXTRACTCORNER", "PCEXTRACTCENTERLINE", "PCEXTRACTSECTION", "-PCEXTRACTSECTION"]
});
