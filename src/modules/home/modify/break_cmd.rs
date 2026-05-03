// BREAK command — remove a portion of a Line, Arc, Circle, or LwPolyline.
//
// Workflow:
//   1. Click to select the entity AND set the first break point.
//   2. Click a second break point.
//   The segment between the two points (going CCW for arcs/circles) is removed.
//
//   BREAK @ (at-sign as second point) → Break at a single point (splits without gap).

use acadrust::entities::{Arc as ArcEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Spline as SplineEnt};
use truck_modeling::base::{BoundedCurve, Cut};
use crate::modules::home::modify::spline_ops::{bspline_to_spline, spline_nearest_t, spline_to_bspline};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::wire_model::WireModel;

// ── Ribbon definition ──────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "BREAK",
        label: "Break",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/trim.svg")),
        event: ModuleEvent::Command("BREAK".to_string()),
    }
}

// ── Geometry ───────────────────────────────────────────────────────────────

/// Break `entity` between world-space points `p1` and `p2`.
/// Returns the replacement entities (empty vec means "erase, no replacement").
pub fn break_entity(entity: &EntityType, p1: Vec3, p2: Vec3) -> Option<Vec<EntityType>> {
    match entity {
        EntityType::Line(line) => Some(break_line(line, p1, p2)),
        EntityType::Arc(arc)   => Some(break_arc(arc, p1, p2)),
        EntityType::Circle(c)  => Some(break_circle(c, p1, p2)),
        EntityType::LwPolyline(p) => Some(break_lwpolyline(p, p1, p2)),
        EntityType::Ellipse(e)    => Some(break_ellipse(e, p1, p2)),
        EntityType::Spline(s)     => Some(break_spline(s, p1, p2)),
        _ => None,
    }
}

fn break_line(line: &LineEnt, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    let s = Vec3::new(line.start.x as f32, line.start.z as f32, line.start.y as f32);
    let e = Vec3::new(line.end.x as f32, line.end.z as f32, line.end.y as f32);
    let dir = e - s;
    let len2 = dir.length_squared();
    if len2 < 1e-12 {
        return vec![];
    }
    let t1 = (p1 - s).dot(dir) / len2;
    let t2 = (p2 - s).dot(dir) / len2;
    let (ta, tb) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
    let ta = ta.clamp(0.0, 1.0);
    let tb = tb.clamp(0.0, 1.0);

    // Single-point break (ta ≈ tb): split into two coincident-endpoint lines
    let pa = world_to_dxf(s + dir * ta);
    let pb = world_to_dxf(s + dir * tb);
    let start = world_to_dxf(s);
    let end = world_to_dxf(e);

    let mut result = Vec::new();
    // First segment: start → pa
    if (pa - start).length() > 1e-6 {
        let mut ent = line.clone();
        ent.common.handle = Handle::NULL;
        ent.start = vec3_to_v3(start);
        ent.end = vec3_to_v3(pa);
        result.push(EntityType::Line(ent));
    }
    // Second segment: pb → end
    if (end - pb).length() > 1e-6 {
        let mut ent = line.clone();
        ent.common.handle = Handle::NULL;
        ent.start = vec3_to_v3(pb);
        ent.end = vec3_to_v3(end);
        result.push(EntityType::Line(ent));
    }
    result
}

fn break_arc(arc: &ArcEnt, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    let cx = arc.center.x as f32;
    let cy = arc.center.z as f32; // Y-up: DXF Y→world Z
    let r = arc.radius as f32;

    // Project p1 and p2 onto the arc (use XZ plane)
    let a1 = angle_on_arc(cx, cy, p1);
    let a2 = angle_on_arc(cx, cy, p2);

    let start = arc.start_angle as f32;
    let end = arc.end_angle as f32;

    // Normalize: clamp a1 to arc range, then remove CCW from a1 to a2
    let a1_on = clamp_to_arc(a1, start, end);
    let a2_on = clamp_to_arc(a2, start, end);

    // Resulting arc: from a2_on to a1_on (CCW, skipping the removed segment)
    // This matches AutoCAD's break behavior: removes CCW from first to second point.
    if (a1_on - a2_on).abs() < 0.01 {
        // Single-point break: return original unchanged (no gap)
        return vec![EntityType::Arc(arc.clone())];
    }

    let _ = r; // radius unchanged
    let mut result = arc.clone();
    result.common.handle = Handle::NULL;
    result.start_angle = a2_on as f64;
    result.end_angle = a1_on as f64;
    vec![EntityType::Arc(result)]
}

fn break_circle(circle: &acadrust::entities::Circle, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    let cx = circle.center.x as f32;
    let cy = circle.center.z as f32;

    let a1 = angle_on_arc(cx, cy, p1);
    let a2 = angle_on_arc(cx, cy, p2);

    if (a1 - a2).abs() < 0.01 {
        return vec![EntityType::Circle(circle.clone())];
    }

    // Convert circle to arc, removing CCW from a1 to a2
    let mut arc = ArcEnt::new();
    arc.common = circle.common.clone();
    arc.common.handle = Handle::NULL;
    arc.center = circle.center.clone();
    arc.radius = circle.radius;
    arc.normal = circle.normal.clone();
    arc.start_angle = a2 as f64;
    arc.end_angle = a1 as f64;
    vec![EntityType::Arc(arc)]
}

fn break_lwpolyline(p: &LwPolyline, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    // For LwPolyline, find the nearest vertex indices for p1 and p2,
    // then split into two polylines at those vertices.
    let n = p.vertices.len();
    if n < 2 {
        return vec![EntityType::LwPolyline(p.clone())];
    }

    let t1 = nearest_pline_param(p, p1);
    let t2 = nearest_pline_param(p, p2);
    let (ta, tb) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };

    // Build two polylines: [0..ta] and [tb..end]
    let idx_a = ta.min(n - 1);
    let idx_b = tb.min(n - 1);

    let mut result = Vec::new();

    // First piece
    if idx_a > 0 {
        let mut first = p.clone();
        first.common.handle = Handle::NULL;
        first.vertices = p.vertices[..=idx_a].to_vec();
        first.is_closed = false;
        result.push(EntityType::LwPolyline(first));
    }

    // Second piece
    if idx_b < n - 1 {
        let mut second = p.clone();
        second.common.handle = Handle::NULL;
        second.vertices = p.vertices[idx_b..].to_vec();
        second.is_closed = false;
        result.push(EntityType::LwPolyline(second));
    }

    if result.is_empty() {
        vec![EntityType::LwPolyline(p.clone())]
    } else {
        result
    }
}

fn break_ellipse(ell: &EllipseEnt, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    // Compute the eccentric-anomaly parameter of a world point relative to ellipse.
    // World coords use XZ plane (Y-up), so DXF X→world X, DXF Y→world Z.
    let cx = ell.center.x;
    let cy = ell.center.y;
    let a = (ell.major_axis.x.powi(2) + ell.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 { return vec![EntityType::Ellipse(ell.clone())]; }
    let _b = a * ell.minor_axis_ratio;
    let nx = ell.major_axis.x / a;
    let ny = ell.major_axis.y / a;

    // Project a point onto the ellipse parameter (eccentric anomaly)
    let param_of = |pt: Vec3| -> f64 {
        let rx = pt.x as f64 - cx;
        let ry = pt.z as f64 - cy; // Y-up: DXF Y → world Z
        let xl =  rx * nx + ry * ny;
        let yl = -rx * ny + ry * nx;
        yl.atan2(xl) // atan2(yl/b*b, xl/a*a) simplifies to atan2(yl,xl) for ordering
    };

    let t0 = ell.start_parameter;
    let t1 = ell.end_parameter;

    let pa1 = param_of(p1);
    let pa2 = param_of(p2);

    // Clamp both params to arc range (same logic as clamp_to_arc for arcs)
    let span_deg = {
        let s = t1 - t0;
        if s <= 0.0 { s + std::f64::consts::TAU } else { s }
    };
    let clamp = |a: f64| -> f64 {
        let rel = ((a - t0) % std::f64::consts::TAU + std::f64::consts::TAU) % std::f64::consts::TAU;
        if rel <= span_deg { a } else if rel < span_deg + (std::f64::consts::TAU - span_deg) / 2.0 { t1 } else { t0 }
    };
    let a1_on = clamp(pa1);
    let a2_on = clamp(pa2);

    if (a1_on - a2_on).abs() < 1e-4 {
        return vec![EntityType::Ellipse(ell.clone())];
    }

    // Remove CCW from a1_on to a2_on → result goes from a2_on to a1_on
    let mut result = ell.clone();
    result.common.handle = Handle::NULL;
    result.start_parameter = a2_on;
    result.end_parameter   = a1_on;
    vec![EntityType::Ellipse(result)]
}

// ── Small utilities ────────────────────────────────────────────────────────

/// Returns the angle (radians, 0-2π) of `pt` viewed from (cx, cy) in the XZ plane.
fn angle_on_arc(cx: f32, cy: f32, pt: Vec3) -> f32 {
    let dx = pt.x - cx;
    let dy = pt.z - cy; // XZ plane
    dy.atan2(dx).rem_euclid(std::f32::consts::TAU)
}

/// Clamp angle `a` to within the arc's angular range (CCW from `start` to `end`).
fn clamp_to_arc(a: f32, start: f32, end: f32) -> f32 {
    let span = (end - start).rem_euclid(std::f32::consts::TAU);
    let rel = (a - start).rem_euclid(std::f32::consts::TAU);
    if rel <= span {
        a
    } else if rel < span + (std::f32::consts::TAU - span) / 2.0 {
        end
    } else {
        start
    }
}

/// Find the index of the polyline vertex closest to `pt`.
fn nearest_pline_param(p: &LwPolyline, pt: Vec3) -> usize {
    p.vertices.iter().enumerate().min_by_key(|(_, v)| {
        let dx = v.location.x as f32 - pt.x;
        let dy = v.location.y as f32 - pt.z;
        ((dx * dx + dy * dy) * 1e6) as i64
    }).map(|(i, _)| i).unwrap_or(0)
}

fn world_to_dxf(v: Vec3) -> Vec3 {
    // world Y-up: X→X, Z→DXF Y, Y→DXF Z
    Vec3::new(v.x, v.z, v.y)
}

fn vec3_to_v3(v: Vec3) -> Vector3 {
    Vector3::new(v.x as f64, v.y as f64, v.z as f64)
}

fn break_spline(spl: &SplineEnt, p1: Vec3, p2: Vec3) -> Vec<EntityType> {
    // Find the two nearest parameters to p1 and p2 (DXF XY: world x, z).
    let t1 = match spline_nearest_t(spl, p1.x as f64, p1.z as f64) {
        Some(t) => t,
        None => return vec![EntityType::Spline(spl.clone())],
    };
    let t2 = match spline_nearest_t(spl, p2.x as f64, p2.z as f64) {
        Some(t) => t,
        None => return vec![EntityType::Spline(spl.clone())],
    };

    let bs = match spline_to_bspline(spl) {
        Some(b) => b,
        None => return vec![EntityType::Spline(spl.clone())],
    };
    let (t0, t_end) = bs.range_tuple();

    let (ta, tb) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };

    // Single-point break (ta ≈ tb): split into two segments at that point.
    if (tb - ta).abs() < 1e-9 {
        let mut piece = bs.clone();
        let right = piece.cut(ta);
        return vec![
            EntityType::Spline(bspline_to_spline(&piece, spl)),
            EntityType::Spline(bspline_to_spline(&right, spl)),
        ];
    }

    // Two-point break: keep [t0..ta] and [tb..t_end], discard middle.
    let mut result = vec![];
    // Left piece [t0, ta]
    if ta - t0 > 1e-9 {
        let mut left = bs.clone();
        let _right = left.cut(ta);   // left = [t0, ta]
        result.push(EntityType::Spline(bspline_to_spline(&left, spl)));
    }
    // Right piece [tb, t_end]
    if t_end - tb > 1e-9 {
        let mut full = bs.clone();
        let right = full.cut(tb);    // right = [tb, t_end]
        result.push(EntityType::Spline(bspline_to_spline(&right, spl)));
    }
    result
}

// ── CadCommand (simplified — break logic via CmdResult::BreakEntity) ───────

/// Thin wrapper for commands.rs to register the break command using the
/// BreakEntity CmdResult variant added below.
pub struct BreakInteractiveCommand {
    target: Option<Handle>,
    p1: Option<Vec3>,
}

impl BreakInteractiveCommand {
    pub fn new() -> Self {
        Self { target: None, p1: None }
    }
}

impl CadCommand for BreakInteractiveCommand {
    fn name(&self) -> &'static str { "BREAK" }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            "BREAK  Select object:".into()
        } else if self.p1.is_none() {
            "BREAK  Specify first break point:".into()
        } else {
            "BREAK  Specify second break point:".into()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: Vec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        self.p1 = Some(pt);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: Vec3) -> CmdResult {
        let handle = match self.target {
            Some(h) => h,
            None => return CmdResult::Cancel,
        };
        let p1 = match self.p1 {
            Some(p) => p,
            None => {
                self.p1 = Some(pt);
                return CmdResult::NeedPoint;
            }
        };
        CmdResult::BreakEntity { handle, p1, p2: pt }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, _pt: Vec3) -> Option<WireModel> {
        None
    }
}

// ── BREAKATPOINT (BAP) — split at a single point, no gap ──────────────────

pub struct BreakAtPointCommand {
    target: Option<Handle>,
}

impl BreakAtPointCommand {
    pub fn new() -> Self {
        Self { target: None }
    }
}

impl CadCommand for BreakAtPointCommand {
    fn name(&self) -> &'static str { "BREAKATPOINT" }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            "BREAKATPOINT  Select object:".into()
        } else {
            "BREAKATPOINT  Specify break point:".into()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: Vec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: Vec3) -> CmdResult {
        match self.target {
            Some(handle) => CmdResult::BreakEntity { handle, p1: pt, p2: pt },
            None => CmdResult::Cancel,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
