// DIVIDE command — place Point entities at N equal intervals along an entity.
// MEASURE command — place Point entities at fixed-distance intervals along an entity.

use std::f64::consts::PI;

use acadrust::entities::Point as PointEnt;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::{CadCommand, CmdResult};

// ── DIVIDE ─────────────────────────────────────────────────────────────────

pub struct DivideCommand {
    target: Option<Handle>,
    waiting_for_n: bool,
}

impl DivideCommand {
    pub fn new() -> Self {
        Self { target: None, waiting_for_n: false }
    }
}

impl CadCommand for DivideCommand {
    fn name(&self) -> &'static str { "DIVIDE" }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            "DIVIDE  Select object to divide:".into()
        } else {
            "DIVIDE  Enter number of segments:".into()
        }
    }

    fn needs_entity_pick(&self) -> bool { self.target.is_none() }

    fn on_entity_pick(&mut self, handle: Handle, _pt: Vec3) -> CmdResult {
        if handle.is_null() { return CmdResult::NeedPoint; }
        self.target = Some(handle);
        self.waiting_for_n = true;
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool { self.waiting_for_n }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let n: usize = text.trim().parse().ok().filter(|&n| n >= 2)?;
        let handle = self.target?;
        self.waiting_for_n = false;
        Some(CmdResult::DivideEntity { handle, n })
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
}

// ── MEASURE ────────────────────────────────────────────────────────────────

pub struct MeasureCommand {
    target: Option<Handle>,
    waiting_for_dist: bool,
}

impl MeasureCommand {
    pub fn new() -> Self {
        Self { target: None, waiting_for_dist: false }
    }
}

impl CadCommand for MeasureCommand {
    fn name(&self) -> &'static str { "MEASURE" }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            "MEASURE  Select object to measure:".into()
        } else {
            "MEASURE  Specify segment length:".into()
        }
    }

    fn needs_entity_pick(&self) -> bool { self.target.is_none() }

    fn on_entity_pick(&mut self, handle: Handle, _pt: Vec3) -> CmdResult {
        if handle.is_null() { return CmdResult::NeedPoint; }
        self.target = Some(handle);
        self.waiting_for_dist = true;
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool { self.waiting_for_dist }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let dist: f64 = text.trim().replace(',', ".").parse().ok().filter(|&d: &f64| d > 0.0)?;
        let handle = self.target?;
        self.waiting_for_dist = false;
        Some(CmdResult::MeasureEntity { handle, segment_length: dist })
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
}

// ── Geometry ───────────────────────────────────────────────────────────────

/// Compute N-1 equally spaced points along the entity (DIVIDE).
pub fn divide_entity(entity: &EntityType, n: usize) -> Vec<EntityType> {
    if n < 2 { return vec![]; }
    let total = entity_length(entity);
    if total < 1e-10 { return vec![]; }
    let step = total / n as f64;
    (1..n).filter_map(|k| {
        let t = step * k as f64;
        point_at_distance(entity, t).map(make_point)
    }).collect()
}

/// Compute points at fixed `segment_length` intervals along the entity (MEASURE).
pub fn measure_entity(entity: &EntityType, segment_length: f64) -> Vec<EntityType> {
    if segment_length <= 0.0 { return vec![]; }
    let total = entity_length(entity);
    if total < 1e-10 { return vec![]; }
    let mut pts = Vec::new();
    let mut t = segment_length;
    while t < total - 1e-6 {
        if let Some(p) = point_at_distance(entity, t) {
            pts.push(make_point(p));
        }
        t += segment_length;
    }
    pts
}

fn make_point(pos: Vector3) -> EntityType {
    let mut p = PointEnt::new();
    p.location = pos;
    EntityType::Point(p)
}

fn entity_length(entity: &EntityType) -> f64 {
    match entity {
        EntityType::Line(l) => {
            let dx = l.end.x - l.start.x;
            let dy = l.end.y - l.start.y;
            let dz = l.end.z - l.start.z;
            (dx*dx + dy*dy + dz*dz).sqrt()
        }
        EntityType::Arc(a) => {
            let span = arc_span_rad(a.start_angle, a.end_angle);
            a.radius * span
        }
        EntityType::Circle(c) => 2.0 * PI * c.radius,
        EntityType::LwPolyline(p) => {
            let n = p.vertices.len();
            if n < 2 { return 0.0; }
            let segs = if p.is_closed { n } else { n - 1 };
            (0..segs).map(|i| {
                let v0 = &p.vertices[i];
                let v1 = &p.vertices[(i + 1) % n];
                let dx = v1.location.x - v0.location.x;
                let dy = v1.location.y - v0.location.y;
                (dx*dx + dy*dy).sqrt()
            }).sum()
        }
        _ => 0.0,
    }
}

fn point_at_distance(entity: &EntityType, d: f64) -> Option<Vector3> {
    match entity {
        EntityType::Line(l) => {
            let total = entity_length(entity);
            if total < 1e-10 { return None; }
            let t = (d / total).clamp(0.0, 1.0);
            Some(Vector3::new(
                l.start.x + t * (l.end.x - l.start.x),
                l.start.y + t * (l.end.y - l.start.y),
                l.start.z + t * (l.end.z - l.start.z),
            ))
        }
        EntityType::Arc(a) => {
            let span = arc_span_rad(a.start_angle, a.end_angle);
            let t = d / a.radius; // arc_length = r * theta
            if t > span { return None; }
            let angle = a.start_angle + t;
            Some(Vector3::new(
                a.center.x + a.radius * angle.cos(),
                a.center.y + a.radius * angle.sin(),
                a.center.z,
            ))
        }
        EntityType::Circle(c) => {
            let circumference = 2.0 * PI * c.radius;
            if circumference < 1e-10 { return None; }
            let angle = 2.0 * PI * (d / circumference);
            Some(Vector3::new(
                c.center.x + c.radius * angle.cos(),
                c.center.y + c.radius * angle.sin(),
                c.center.z,
            ))
        }
        EntityType::LwPolyline(p) => {
            let n = p.vertices.len();
            if n < 2 { return None; }
            let segs = if p.is_closed { n } else { n - 1 };
            let mut acc = 0.0f64;
            for i in 0..segs {
                let v0 = &p.vertices[i];
                let v1 = &p.vertices[(i + 1) % n];
                let dx = v1.location.x - v0.location.x;
                let dy = v1.location.y - v0.location.y;
                let seg_len = (dx*dx + dy*dy).sqrt();
                if acc + seg_len >= d - 1e-10 {
                    let t = (d - acc) / seg_len.max(1e-10);
                    return Some(Vector3::new(
                        v0.location.x + t * dx,
                        v0.location.y + t * dy,
                        p.elevation,
                    ));
                }
                acc += seg_len;
            }
            None
        }
        _ => None,
    }
}

fn arc_span_rad(start: f64, end: f64) -> f64 {
    let span = (end - start).rem_euclid(2.0 * PI);
    if span < 1e-6 { 2.0 * PI } else { span }
}
