// Offset tool — ribbon definition + interactive command.
//
// Command:  OFFSET (O)
//   OFFSET: Creates a parallel copy of an object (line, arc, circle,
//   or lwpolyline) at a specified distance on the chosen side.
//
//   Steps:
//     1. Text input: "Specify offset distance <last>:" → enter float or Enter for default
//     2. Pick object to offset (Line, Arc, Circle, LwPolyline)
//     3. Pick a point on the side to offset toward

use std::f64::consts::TAU;

use acadrust::entities::LwVertex;
use acadrust::entities::{Arc as ArcEnt, Circle as CircleEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Spline as SplineEnt};
use crate::modules::home::modify::spline_ops::{spline_sample_xy, spline_pts_wire};
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::home::defaults;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::wire_model::WireModel;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "OFFSET",
        label: "Offset",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/offset.svg")),
        event: ModuleEvent::Command("OFFSET".to_string()),
    }
}

// ── Geometry helpers ────────────────────────────────────────────────────────

/// Infinite-line intersection in 2D.  Returns the point or None if parallel.
fn isect_lines(p0: [f64; 2], p1: [f64; 2], q0: [f64; 2], q1: [f64; 2]) -> Option<[f64; 2]> {
    let dx = p1[0] - p0[0];
    let dy = p1[1] - p0[1];
    let ex = q1[0] - q0[0];
    let ey = q1[1] - q0[1];
    let det = dx * ey - dy * ex;
    if det.abs() < 1e-10 {
        return None;
    }
    let t = ((q0[0] - p0[0]) * ey - (q0[1] - p0[1]) * ex) / det;
    Some([p0[0] + t * dx, p0[1] + t * dy])
}

fn norm_rad(a: f64) -> f64 {
    ((a % TAU) + TAU) % TAU
}

// ── Line offset ────────────────────────────────────────────────────────────

fn offset_line(l: &LineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let dx = l.end.x - l.start.x;
    let dy = l.end.y - l.start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 {
        return None;
    }

    let nx = -dy / len; // left-perpendicular
    let ny = dx / len;

    let vx = side_pt.x as f64 - l.start.x;
    let vy = side_pt.y as f64 - l.start.y;
    let cross = dx * vy - dy * vx;
    let sign = if cross >= 0.0 { 1.0 } else { -1.0 };

    let ox = sign * nx * dist;
    let oy = sign * ny * dist;

    let mut new_l = l.clone();
    new_l.common.handle = Handle::NULL;
    new_l.start.x += ox;
    new_l.start.y += oy;
    new_l.end.x += ox;
    new_l.end.y += oy;
    Some(EntityType::Line(new_l))
}

// ── Circle offset ──────────────────────────────────────────────────────────

fn offset_circle(c: &CircleEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let px = side_pt.x as f64;
    let py = side_pt.y as f64;
    let dc = ((px - c.center.x).powi(2) + (py - c.center.y).powi(2)).sqrt();

    let new_r = if dc < c.radius {
        c.radius - dist
    } else {
        c.radius + dist
    };
    if new_r <= 1e-9 {
        return None;
    }

    let mut new_c = c.clone();
    new_c.common.handle = Handle::NULL;
    new_c.radius = new_r;
    Some(EntityType::Circle(new_c))
}

// ── Arc offset ─────────────────────────────────────────────────────────────

fn offset_arc(a: &ArcEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let px = side_pt.x as f64;
    let py = side_pt.y as f64;
    let dc = ((px - a.center.x).powi(2) + (py - a.center.y).powi(2)).sqrt();

    let new_r = if dc < a.radius {
        a.radius - dist
    } else {
        a.radius + dist
    };
    if new_r <= 1e-9 {
        return None;
    }

    let mut new_a = a.clone();
    new_a.common.handle = Handle::NULL;
    new_a.radius = new_r;
    Some(EntityType::Arc(new_a))
}

// ── LwPolyline offset ──────────────────────────────────────────────────────
//
// Algorithm:
//   1. Offset every segment by `dist` in the direction perpendicular to it
//      (sign is determined once from the first non-degenerate segment + side_pt).
//   2. Reconnect adjacent offset segments:
//      - Open: first / last vertex use the raw offset endpoints;
//        interior vertices are the intersection of adjacent offset segments.
//      - Closed: every vertex is the intersection of the previous and next
//        offset segments.
//   3. Bulge values are preserved from the original vertices (arc segments
//      keep the same angle; the radius changes implicitly via the new chord
//      length — a minor approximation acceptable for modest offsets).

fn offset_lwpolyline(p: &LwPolyline, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return None;
    }

    let n_segs = if p.is_closed { n } else { n - 1 };

    // Determine offset sign from the first non-degenerate segment.
    let sign: f64 = (0..n_segs).find_map(|i| {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let dx = v1.location.x - v0.location.x;
        let dy = v1.location.y - v0.location.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            return None;
        }
        let vx = side_pt.x as f64 - v0.location.x;
        let vy = side_pt.y as f64 - v0.location.y;
        let cross = dx * vy - dy * vx;
        Some(if cross >= 0.0 { 1.0 } else { -1.0 })
    })?;

    // Offset each segment.  A segment may be degenerate (zero length) → None.
    struct OffSeg {
        p0: [f64; 2],
        p1: [f64; 2],
    }

    let segs: Vec<Option<OffSeg>> = (0..n_segs)
        .map(|i| {
            let v0 = &p.vertices[i];
            let v1 = &p.vertices[(i + 1) % n];
            let dx = v1.location.x - v0.location.x;
            let dy = v1.location.y - v0.location.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-12 {
                return None;
            }
            let ox = sign * (-dy / len) * dist;
            let oy = sign * (dx / len) * dist;
            Some(OffSeg {
                p0: [v0.location.x + ox, v0.location.y + oy],
                p1: [v1.location.x + ox, v1.location.y + oy],
            })
        })
        .collect();

    let m = segs.len();

    // Helper: corner vertex from the intersection of two consecutive offset segments.
    let corner = |prev: &OffSeg, curr: &OffSeg| -> [f64; 2] {
        isect_lines(prev.p0, prev.p1, curr.p0, curr.p1).unwrap_or([
            (prev.p1[0] + curr.p0[0]) * 0.5,
            (prev.p1[1] + curr.p0[1]) * 0.5,
        ])
    };

    let mut new_verts: Vec<LwVertex> = Vec::new();

    if p.is_closed {
        for i in 0..m {
            let prev_idx = (i + m - 1) % m;
            let prev = match &segs[prev_idx] {
                Some(s) => s,
                None => continue,
            };
            let curr = match &segs[i] {
                Some(s) => s,
                None => continue,
            };
            let pt = corner(prev, curr);
            let mut v = LwVertex::from_coords(pt[0], pt[1]);
            v.bulge = p.vertices[i].bulge;
            new_verts.push(v);
        }
    } else {
        // First vertex
        if let Some(s) = &segs[0] {
            let mut v = LwVertex::from_coords(s.p0[0], s.p0[1]);
            v.bulge = p.vertices[0].bulge;
            new_verts.push(v);
        }
        // Interior vertices
        for i in 1..m {
            let prev = match &segs[i - 1] {
                Some(s) => s,
                None => continue,
            };
            let curr = match &segs[i] {
                Some(s) => s,
                None => continue,
            };
            let pt = corner(prev, curr);
            let mut v = LwVertex::from_coords(pt[0], pt[1]);
            v.bulge = p.vertices[i].bulge;
            new_verts.push(v);
        }
        // Last vertex
        if let Some(s) = &segs[m - 1] {
            new_verts.push(LwVertex::from_coords(s.p1[0], s.p1[1]));
        }
    }

    if new_verts.len() < 2 {
        return None;
    }

    let mut new_p = p.clone();
    new_p.common.handle = Handle::NULL;
    new_p.vertices = new_verts;
    Some(EntityType::LwPolyline(new_p))
}

// ── Ellipse offset ─────────────────────────────────────────────────────────
//
// A true offset of an ellipse is a Lamé curve, not an ellipse. As an
// acceptable CAD approximation we scale both semi-axes uniformly and keep
// the same orientation, center and parameter range.  The sign of the offset
// is determined by whether side_pt is inside or outside the ellipse.

fn offset_ellipse(e: &EllipseEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 { return None; }
    let b = a * e.minor_axis_ratio;
    let nx = e.major_axis.x / a;
    let ny = e.major_axis.y / a;
    // Project side_pt onto ellipse local frame and test inside/outside.
    let rx = side_pt.x as f64 - e.center.x;
    let ry = side_pt.y as f64 - e.center.y;
    let xl =  rx * nx + ry * ny;
    let yl = -rx * ny + ry * nx;
    let inside = (xl / a).powi(2) + (yl / b).powi(2) < 1.0;
    let sign = if inside { -1.0 } else { 1.0 };

    let new_a = a + sign * dist;
    let new_b = b + sign * dist;
    if new_a <= 1e-9 || new_b <= 1e-9 { return None; }

    let mut new_e = e.clone();
    new_e.common.handle = Handle::NULL;
    // Scale the major_axis vector proportionally.
    let scale = new_a / a;
    new_e.major_axis.x *= scale;
    new_e.major_axis.y *= scale;
    new_e.major_axis.z *= scale;
    new_e.minor_axis_ratio = new_b / new_a;
    Some(EntityType::Ellipse(new_e))
}

// ── Spline offset ──────────────────────────────────────────────────────────
//
// Strategy: sample the spline into N points, offset each sample point by
// `dist` along the local perpendicular (based on the finite-difference
// tangent), then fit a new spline through the offset points.

fn offset_spline(spl: &SplineEnt, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    let (ts_knot, pts) = spline_sample_xy(spl, 64);
    let n = pts.len();
    if n < 2 { return None; }

    // Determine offset sign from the first non-degenerate tangent.
    let sign: f64 = (0..n - 1).find_map(|i| {
        let dx = pts[i + 1][0] - pts[i][0];
        let dy = pts[i + 1][1] - pts[i][1];
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 { return None; }
        let vx = side_pt.x as f64 - pts[i][0];
        let vy = side_pt.y as f64 - pts[i][1];
        let cross = dx * vy - dy * vx;
        Some(if cross >= 0.0 { 1.0 } else { -1.0 })
    })?;

    // Offset each sample point along the local normal.
    let offset_pts: Vec<acadrust::types::Vector3> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            // Tangent via central / forward / backward difference.
            let (dx, dy) = if i == 0 {
                let d = [pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]];
                (d[0], d[1])
            } else if i == n - 1 {
                let d = [pts[n - 1][0] - pts[n - 2][0], pts[n - 1][1] - pts[n - 2][1]];
                (d[0], d[1])
            } else {
                ((pts[i + 1][0] - pts[i - 1][0]) * 0.5,
                 (pts[i + 1][1] - pts[i - 1][1]) * 0.5)
            };
            let len = (dx * dx + dy * dy).sqrt().max(1e-12);
            let nx = -dy / len;  // left perpendicular
            let ny =  dx / len;
            let z = spl.control_points.first().map(|v| v.z).unwrap_or(0.0);
            acadrust::types::Vector3::new(
                p[0] + sign * nx * dist,
                p[1] + sign * ny * dist,
                z,
            )
        })
        .collect();

    let _ = ts_knot;
    // Build a new spline from the offset control points (treat sample pts as fit pts → ctrl pts).
    let degree = spl.degree.max(1) as usize;
    let new_ctrl: Vec<acadrust::types::Vector3> = offset_pts;
    let n_ctrl = new_ctrl.len();
    let kv = truck_modeling::KnotVec::uniform_knot(degree, n_ctrl - 1);
    let mut new_spl = spl.clone();
    new_spl.common.handle = Handle::NULL;
    new_spl.control_points = new_ctrl;
    new_spl.knots = kv.iter().copied().collect();
    new_spl.fit_points.clear();
    new_spl.weights.clear();
    Some(EntityType::Spline(new_spl))
}

// ── Dispatch ───────────────────────────────────────────────────────────────

fn compute_offset(entity: &EntityType, dist: f64, side_pt: Vec3) -> Option<EntityType> {
    match entity {
        EntityType::Line(l) => offset_line(l, dist, side_pt),
        EntityType::Circle(c) => offset_circle(c, dist, side_pt),
        EntityType::Arc(a) => offset_arc(a, dist, side_pt),
        EntityType::LwPolyline(p) => offset_lwpolyline(p, dist, side_pt),
        EntityType::Ellipse(e) => offset_ellipse(e, dist, side_pt),
        EntityType::Spline(s) => offset_spline(s, dist, side_pt),
        _ => None,
    }
}

// ── Wire preview points ─────────────────────────────────────────────────────

fn entity_wire_pts(e: &EntityType) -> Vec<[f32; 3]> {
    match e {
        EntityType::Line(l) => vec![
            [l.start.x as f32, l.start.y as f32, l.start.z as f32],
            [l.end.x as f32, l.end.y as f32, l.end.z as f32],
        ],
        EntityType::Circle(c) => {
            let steps = 64usize;
            (0..=steps)
                .map(|i| {
                    let a = TAU * i as f64 / steps as f64;
                    [
                        (c.center.x + c.radius * a.cos()) as f32,
                        (c.center.y + c.radius * a.sin()) as f32,
                        c.center.z as f32,
                    ]
                })
                .collect()
        }
        EntityType::Arc(a) => {
            let a0 = norm_rad(a.start_angle);
            let a1 = norm_rad(a.end_angle);
            let span = {
                let s = a1 - a0;
                if s <= 0.0 {
                    s + TAU
                } else {
                    s
                }
            };
            let steps = ((span.abs() * 20.0).ceil() as usize).max(4);
            (0..=steps)
                .map(|i| {
                    let ang = a0 + span * (i as f64 / steps as f64);
                    [
                        (a.center.x + a.radius * ang.cos()) as f32,
                        (a.center.y + a.radius * ang.sin()) as f32,
                        a.center.z as f32,
                    ]
                })
                .collect()
        }
        EntityType::LwPolyline(p) => lwpolyline_pts(p),
        EntityType::Ellipse(e) => {
            let a = (e.major_axis.x.powi(2) + e.major_axis.y.powi(2)).sqrt();
            if a < 1e-9 { return vec![]; }
            let b = a * e.minor_axis_ratio;
            let nx = e.major_axis.x / a;
            let ny = e.major_axis.y / a;
            let t0 = e.start_parameter;
            let mut t1 = e.end_parameter;
            if t1 <= t0 { t1 += TAU; }
            let span = t1 - t0;
            let steps = ((span.abs() * 20.0).ceil() as usize).max(4);
            (0..=steps).map(|i| {
                let t = t0 + span * (i as f64 / steps as f64);
                let lx = a * t.cos();
                let ly = b * t.sin();
                [(e.center.x + lx * nx - ly * ny) as f32,
                 (e.center.y + lx * ny + ly * nx) as f32,
                 e.center.z as f32]
            }).collect()
        }
        EntityType::Spline(s) => spline_pts_wire(s),
        _ => vec![],
    }
}

/// Tessellate a LwPolyline into wire points (straight segments + arc bulges).
fn lwpolyline_pts(p: &LwPolyline) -> Vec<[f32; 3]> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let z = p.elevation as f32;
    let n_segs = if p.is_closed { n } else { n - 1 };
    let mut pts: Vec<[f32; 3]> = Vec::new();

    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let x0 = v0.location.x;
        let y0 = v0.location.y;
        let x1 = v1.location.x;
        let y1 = v1.location.y;

        if pts.is_empty() {
            pts.push([x0 as f32, y0 as f32, z]);
        }

        if v0.bulge.abs() < 1e-10 {
            pts.push([x1 as f32, y1 as f32, z]);
        } else {
            // Arc from bulge
            let b = v0.bulge;
            let chord_x = x1 - x0;
            let chord_y = y1 - y0;
            let chord_len = (chord_x * chord_x + chord_y * chord_y).sqrt();
            if chord_len < 1e-12 {
                pts.push([x1 as f32, y1 as f32, z]);
                continue;
            }

            let b2 = b * b;
            let r = chord_len * (1.0 + b2) / (4.0 * b.abs());
            let d = r * (1.0 - b2) / (1.0 + b2);
            let mx = (x0 + x1) * 0.5;
            let my = (y0 + y1) * 0.5;
            let perp_x = -chord_y / chord_len;
            let perp_y = chord_x / chord_len;
            let sign = b.signum();
            let cx = mx + sign * d * perp_x;
            let cy = my + sign * d * perp_y;

            let a0 = norm_rad((y0 - cy).atan2(x0 - cx));
            let a1 = norm_rad((y1 - cy).atan2(x1 - cx));
            let span = if b > 0.0 {
                let s = a1 - a0;
                if s <= 0.0 {
                    s + TAU
                } else {
                    s
                }
            } else {
                let s = a0 - a1;
                if s <= 0.0 {
                    s + TAU
                } else {
                    s
                }
            };
            let steps = ((span.abs() * 20.0).ceil() as usize).max(4);
            for j in 1..=steps {
                let t = j as f64 / steps as f64;
                let ang = if b > 0.0 {
                    a0 + span * t
                } else {
                    a0 - span * t
                };
                pts.push([(cx + r * ang.cos()) as f32, (cy + r * ang.sin()) as f32, z]);
            }
        }
    }

    if p.is_closed {
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
    }
    pts
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    Distance,
    SelectObject {
        dist: f64,
    },
    PickSide {
        dist: f64,
        #[allow(dead_code)]
        handle: Handle,
        entity: EntityType,
    },
}

pub struct OffsetCommand {
    default_dist: f64,
    step: Step,
    all_entities: Vec<EntityType>,
}

impl OffsetCommand {
    pub fn new(last_dist: f32, all_entities: Vec<EntityType>) -> Self {
        Self {
            default_dist: last_dist as f64,
            step: Step::Distance,
            all_entities,
        }
    }
}

impl CadCommand for OffsetCommand {
    fn name(&self) -> &'static str {
        "OFFSET"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Distance => format!(
                "OFFSET  Specify offset distance <{:.4}>:",
                self.default_dist
            ),
            Step::SelectObject { dist } => {
                format!("OFFSET  Select object to offset  [dist={dist:.4}]:")
            }
            Step::PickSide { dist, .. } => {
                format!("OFFSET  Specify side to offset  [dist={dist:.4}]:")
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Distance)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Step::Distance = self.step {
            let dist = if text.trim().is_empty() {
                self.default_dist
            } else {
                text.trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .unwrap_or(self.default_dist)
                    .abs()
                    .max(1e-9)
            };
            defaults::set_offset_dist(dist as f32);
            self.default_dist = dist;
            self.step = Step::SelectObject { dist };
        }
        None
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::SelectObject { .. })
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: Vec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }

        let dist = match &self.step {
            Step::SelectObject { dist } => *dist,
            _ => return CmdResult::NeedPoint,
        };

        let entity = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle)
            .cloned();

        match entity {
            Some(e @ EntityType::Line(_))
            | Some(e @ EntityType::Circle(_))
            | Some(e @ EntityType::Arc(_))
            | Some(e @ EntityType::LwPolyline(_)) => {
                self.step = Step::PickSide {
                    dist,
                    handle,
                    entity: e,
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_hover_entity(&mut self, handle: Handle, _pt: Vec3) -> Vec<WireModel> {
        if handle.is_null() {
            return vec![];
        }
        if let Some(entity) = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle)
        {
            let pts = entity_wire_pts(entity);
            if !pts.is_empty() {
                return vec![WireModel::solid(
                    "offset_hover".into(),
                    pts,
                    WireModel::CYAN,
                    false,
                )];
            }
        }
        vec![]
    }

    fn on_point(&mut self, pt: Vec3) -> CmdResult {
        let (dist, entity) = match &self.step {
            Step::PickSide { dist, entity, .. } => (*dist, entity.clone()),
            _ => return CmdResult::NeedPoint,
        };

        match compute_offset(&entity, dist, pt) {
            Some(new_entity) => CmdResult::CommitAndExit(new_entity),
            None => CmdResult::NeedPoint,
        }
    }

    fn on_preview_wires(&mut self, pt: Vec3) -> Vec<WireModel> {
        let (dist, entity) = match &self.step {
            Step::PickSide { dist, entity, .. } => (*dist, entity.clone()),
            _ => return vec![],
        };

        if let Some(result) = compute_offset(&entity, dist, pt) {
            let pts = entity_wire_pts(&result);
            if !pts.is_empty() {
                return vec![WireModel::solid(
                    "offset_preview".into(),
                    pts,
                    WireModel::CYAN,
                    false,
                )];
            }
        }
        vec![]
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
