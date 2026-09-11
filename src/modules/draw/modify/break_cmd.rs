// BREAK command — remove a portion of a Line, Arc, Circle, or LwPolyline.
//
// Workflow:
//   1. Click to select the entity AND set the first break point.
//   2. Click a second break point.
//   The segment between the two points (going CCW for arcs/circles) is removed.
//
//   BREAK @ (at-sign as second point) → Break at a single point (splits without gap).

use crate::modules::draw::modify::spline_ops::{spline_cut, spline_nearest_t, spline_range};
use acadrust::entities::{
    Arc as ArcEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Spline as SplineEnt,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

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
pub fn break_entity(entity: &EntityType, p1: DVec3, p2: DVec3) -> Option<Vec<EntityType>> {
    match entity {
        EntityType::Line(line) => Some(break_line(line, p1, p2)),
        EntityType::Arc(arc) => Some(break_arc(arc, p1, p2)),
        EntityType::Circle(c) => Some(break_circle(c, p1, p2)),
        EntityType::LwPolyline(p) => {
            let p = crate::entities::curve::lwpolyline_world_xy(p)?;
            Some(break_lwpolyline(&p, p1, p2))
        }
        EntityType::Ellipse(e) => Some(break_ellipse(e, p1, p2)),
        EntityType::Spline(s) => Some(break_spline(s, p1, p2)),
        _ => None,
    }
}

fn break_line(line: &LineEnt, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
    let s = DVec3::new(line.start.x, line.start.y, line.start.z);
    let e = DVec3::new(line.end.x, line.end.y, line.end.z);
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

fn picked_spans(curve: &cadkernel::space::PlanarCurve, p1: DVec3, p2: DVec3) -> Option<Vec<[f64; 2]>> {
    let first = curve.plane.project(p1.to_array())?;
    let second = curve.plane.project(p2.to_array())?;
    let a = cadkernel::geom2d::closest_point(&curve.curve, first).t;
    let b = cadkernel::geom2d::closest_point(&curve.curve, second).t;
    cadkernel::geom2d::break_spans(&curve.curve, a, b, cadkernel::geom2d::Tolerance::new(1e-9))
}

fn break_arc(arc: &ArcEnt, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
    let curve = crate::entities::curve::arc_curve(arc);
    let Some(spans) = picked_spans(&curve, p1, p2) else {
        return vec![EntityType::Arc(arc.clone())];
    };
    let cadkernel::geom2d::Curve::Arc(geometry) = &curve.curve else { unreachable!() };
    spans.into_iter().map(|[from, to]| {
        let mut result = arc.clone();
        result.common.handle = Handle::NULL;
        result.start_angle = geometry.start_angle + from * geometry.sweep();
        result.end_angle = geometry.start_angle + to * geometry.sweep();
        EntityType::Arc(result)
    }).collect()
}

fn break_circle(circle: &acadrust::entities::Circle, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
    let curve = crate::entities::curve::circle_curve(circle);
    let Some(spans) = picked_spans(&curve, p1, p2) else {
        return vec![EntityType::Circle(circle.clone())];
    };
    spans.into_iter().map(|[from, to]| {
        let mut arc = ArcEnt::new();
        arc.common = circle.common.clone();
        arc.common.handle = Handle::NULL;
        arc.center = circle.center;
        arc.radius = circle.radius;
        arc.thickness = circle.thickness;
        arc.normal = circle.normal;
        arc.start_angle = from * std::f64::consts::TAU;
        arc.end_angle = (to * std::f64::consts::TAU).rem_euclid(std::f64::consts::TAU);
        EntityType::Arc(arc)
    }).collect()
}
fn break_lwpolyline(p: &LwPolyline, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
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

fn break_ellipse(ell: &EllipseEnt, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
    let Some(curve) = crate::entities::curve::ellipse_curve(ell) else {
        return vec![EntityType::Ellipse(ell.clone())];
    };
    let Some(spans) = picked_spans(&curve, p1, p2) else {
        return vec![EntityType::Ellipse(ell.clone())];
    };
    let cadkernel::geom2d::Curve::Ellipse(geometry) = &curve.curve else { unreachable!() };
    spans.into_iter().map(|[from, to]| {
        let mut result = ell.clone();
        result.common.handle = Handle::NULL;
        result.start_parameter = geometry.start_parameter + from * geometry.sweep();
        result.end_parameter = geometry.start_parameter + to * geometry.sweep();
        EntityType::Ellipse(result)
    }).collect()
}
/// Find the index of the polyline vertex closest to `pt`.
fn nearest_pline_param(p: &LwPolyline, pt: DVec3) -> usize {
    p.vertices
        .iter()
        .enumerate()
        .min_by_key(|(_, v)| {
            let dx = v.location.x - pt.x;
            let dy = v.location.y - pt.y;
            ((dx * dx + dy * dy) * 1e6) as i64
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn world_to_dxf(v: DVec3) -> DVec3 {
    // World = DXF (identity).
    DVec3::new(v.x, v.y, v.z)
}

fn vec3_to_v3(v: DVec3) -> Vector3 {
    Vector3::new(v.x, v.y, v.z)
}

fn break_spline(spl: &SplineEnt, p1: DVec3, p2: DVec3) -> Vec<EntityType> {
    // Find the two nearest parameters to p1 and p2 (DXF XY: world x, z).
    let t1 = match spline_nearest_t(spl, p1.x, p1.y) {
        Some(t) => t,
        None => return vec![EntityType::Spline(spl.clone())],
    };
    let t2 = match spline_nearest_t(spl, p2.x, p2.y) {
        Some(t) => t,
        None => return vec![EntityType::Spline(spl.clone())],
    };

    let Some((t0, t_end)) = spline_range(spl) else {
        return vec![EntityType::Spline(spl.clone())];
    };

    let (ta, tb) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };

    // Single-point break (ta ≈ tb): split into two segments at that point.
    if (tb - ta).abs() < 1e-9 {
        return match spline_cut(spl, ta) {
            Some((left, right)) => {
                vec![EntityType::Spline(left), EntityType::Spline(right)]
            }
            None => vec![EntityType::Spline(spl.clone())],
        };
    }

    // Two-point break: keep [t0..ta] and [tb..t_end], discard middle.
    let mut result = vec![];
    // Left piece [t0, ta]
    if ta - t0 > 1e-9 {
        if let Some((left, _)) = spline_cut(spl, ta) {
            result.push(EntityType::Spline(left));
        }
    }
    // Right piece [tb, t_end]
    if t_end - tb > 1e-9 {
        if let Some((_, right)) = spline_cut(spl, tb) {
            result.push(EntityType::Spline(right));
        }
    }
    result
}

// ── CadCommand (simplified — break logic via CmdResult::BreakEntity) ───────

/// Thin wrapper for commands.rs to register the break command using the
/// BreakEntity CmdResult variant added below.
pub struct BreakInteractiveCommand {
    target: Option<Handle>,
    p1: Option<DVec3>,
}

impl BreakInteractiveCommand {
    pub fn new() -> Self {
        Self {
            target: None,
            p1: None,
        }
    }
}

impl CadCommand for BreakInteractiveCommand {
    fn name(&self) -> &'static str {
        "BREAK"
    }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            crate::t!("BREAK  Select object:").into_owned()
        } else if self.p1.is_none() {
            crate::t!("BREAK  Specify first break point:").into_owned()
        } else {
            crate::t!("BREAK  Specify second break point or [First point]:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        if self.target.is_some() && self.p1.is_some() {
            vec![crate::command::CmdOption::new("First point", "F")]
        } else {
            Vec::new()
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let handle = self.target?;
        match text.trim().to_ascii_uppercase().as_str() {
            "F" | "FIRST" => {
                self.p1 = None;
                Some(CmdResult::NeedPoint)
            }
            "@" => self.p1.map(|point| CmdResult::BreakEntity {
                handle, p1: point, p2: point,
            }),
            _ => None,
        }
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        self.p1 = Some(pt);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
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

    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
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
    fn name(&self) -> &'static str {
        "BREAKATPOINT"
    }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            t!("BREAKATPOINT  Select object:").into_owned()
        } else {
            t!("BREAKATPOINT  Specify break point:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.target = Some(handle);
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.target {
            Some(handle) => CmdResult::BreakEntity {
                handle,
                p1: pt,
                p2: pt,
            },
            None => CmdResult::Cancel,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["BREAKATPOINT"] });  // BreakAtPointCommand
inventory::submit!(crate::command::CommandRegistration { names: &["BREAK"] });  // BreakInteractiveCommand
