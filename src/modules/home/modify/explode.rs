// Explode tool — ribbon definition + command implementation.
//
// Command:  EXPLODE (X)
//   EXPLODE: Breaks compound objects into their constituent simple entities.
//
//   Supported:
//     LwPolyline  → Lines (straight segments) + Arcs (bulge segments)
//     Polyline2D  → Lines + Arcs
//     Polyline3D  → Lines
//     Polyline    → Lines
//     Insert      → constituent entities (via acadrust explode_from_document)
//     MLine       → Lines (spine + offset lines per miter direction)
//     Dimension   → Lines (extension + dimension + arrows) + Text
//
//   Unsupported entity types are skipped silently.

use std::f64::consts::TAU;

use acadrust::entities::EntityCommon;
use acadrust::entities::{
    Arc as ArcEnt, Circle as CircleEnt, Dimension, Line as LineEnt, LwPolyline, MLine,
};
use acadrust::entities::{Polyline, Polyline2D};
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle};

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use glam::Vec3;

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "EXPLODE",
        label: "Explode",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/explode.svg")),
        event: ModuleEvent::Command("EXPLODE".to_string()),
    }
}

// ── Geometry helpers ────────────────────────────────────────────────────────

/// Decompose an entity into its constituent simple entities.
/// Returns an empty vec if the entity cannot be exploded.
pub fn explode_entity(entity: &EntityType, document: &CadDocument) -> Vec<EntityType> {
    match entity {
        EntityType::LwPolyline(p) => explode_lwpolyline(p),
        EntityType::Polyline2D(p) => explode_polyline2d(p),
        EntityType::Polyline(p) => explode_polyline(p),
        EntityType::Polyline3D(p) => explode_polyline3d(p),
        EntityType::Insert(ins) => {
            let is_mirrored = ins.x_scale() * ins.y_scale() < 0.0;
            ins.explode_from_document(document)
                .into_iter()
                .map(normalize_insert_entity)
                .map(|sub| fix_mirrored_arc(sub, is_mirrored))
                .collect()
        }
        EntityType::MLine(ml) => explode_mline(ml),
        EntityType::Dimension(dim) => explode_dimension(dim),
        _ => vec![],
    }
}

fn explode_polyline(p: &Polyline) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.flags.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let mut common = p.common.clone();
        common.handle = Handle::NULL;
        result.push(EntityType::Line(LineEnt {
            common,
            start: v0.location.clone(),
            end: v1.location.clone(),
            ..LineEnt::new()
        }));
    }
    result
}

fn explode_polyline3d(p: &acadrust::entities::Polyline3D) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let mut common = p.common.clone();
        common.handle = Handle::NULL;
        result.push(EntityType::Line(LineEnt {
            common,
            start: v0.position.clone(),
            end: v1.position.clone(),
            ..LineEnt::new()
        }));
    }
    result
}

fn explode_polyline2d(p: &Polyline2D) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = p.is_closed();
    let n_segs = if closed { n } else { n - 1 };
    let elevation = p.elevation;

    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];
        let p0 = [v0.location.x, v0.location.y];
        let p1 = [v1.location.x, v1.location.y];

        if v0.bulge.abs() < 1e-10 {
            let mut common = p.common.clone();
            common.handle = Handle::NULL;
            result.push(EntityType::Line(LineEnt {
                common,
                start: Vector3::new(p0[0], p0[1], elevation),
                end: Vector3::new(p1[0], p1[1], elevation),
                ..LineEnt::new()
            }));
        } else if let Some(arc) = bulge_to_arc(p0, p1, v0.bulge, elevation, &p.common) {
            result.push(arc);
        }
    }
    result
}

pub fn normalize_insert_entity(mut entity: EntityType) -> EntityType {
    match &mut entity {
        EntityType::Ellipse(ell) => {
            let major_len = ell.major_axis_length();
            let full_span = {
                let mut span = ell.end_parameter - ell.start_parameter;
                if span < 0.0 {
                    span += std::f64::consts::TAU;
                }
                (span - std::f64::consts::TAU).abs() < 1e-6
            };
            if (ell.minor_axis_ratio - 1.0).abs() < 1e-6 && full_span {
                let mut circle = CircleEnt::new();
                circle.common = ell.common.clone();
                circle.center = ell.center;
                circle.radius = major_len;
                circle.normal = ell.normal;
                entity = EntityType::Circle(circle);
            }
        }
        _ => {}
    }

    entity.common_mut().handle = Handle::NULL;
    entity.common_mut().owner_handle = Handle::NULL;
    entity
}

pub fn normalize_entity_for_block(entity: EntityType) -> EntityType {
    entity
}

/// Swap arc start/end angles when the INSERT that produced the entity was
/// mirrored (exactly one of x_scale / y_scale is negative).  The acadrust
/// explode recalculates endpoint positions correctly but does not reverse the
/// sweep direction, so the arc would otherwise curve the wrong way.
pub fn fix_mirrored_arc(entity: EntityType, is_mirrored: bool) -> EntityType {
    if !is_mirrored {
        return entity;
    }
    if let EntityType::Arc(mut arc) = entity {
        std::mem::swap(&mut arc.start_angle, &mut arc.end_angle);
        EntityType::Arc(arc)
    } else {
        entity
    }
}

fn explode_lwpolyline(p: &LwPolyline) -> Vec<EntityType> {
    let n = p.vertices.len();
    if n < 2 {
        return vec![];
    }

    let elevation = p.elevation;
    let n_segs = if p.is_closed { n } else { n - 1 };

    let mut result = Vec::new();
    for i in 0..n_segs {
        let v0 = &p.vertices[i];
        let v1 = &p.vertices[(i + 1) % n];

        let p0 = [v0.location.x, v0.location.y];
        let p1 = [v1.location.x, v1.location.y];

        if v0.bulge.abs() < 1e-10 {
            // Straight segment → Line
            let mut common = p.common.clone();
            common.handle = Handle::NULL;
            let line = LineEnt {
                common,
                start: Vector3::new(p0[0], p0[1], elevation),
                end: Vector3::new(p1[0], p1[1], elevation),
                ..LineEnt::new()
            };
            result.push(EntityType::Line(line));
        } else {
            // Arc segment from bulge
            if let Some(arc) = bulge_to_arc(p0, p1, v0.bulge, elevation, &p.common) {
                result.push(arc);
            }
        }
    }
    result
}

/// Convert a polyline bulge segment to an Arc entity.
///   Arc angles are measured from the +X axis.
fn bulge_to_arc(
    p0: [f64; 2],
    p1: [f64; 2],
    bulge: f64,
    elevation: f64,
    common_src: &EntityCommon,
) -> Option<EntityType> {
    let chord_x = p1[0] - p0[0];
    let chord_y = p1[1] - p0[1];
    let chord_len = (chord_x * chord_x + chord_y * chord_y).sqrt();
    if chord_len < 1e-12 {
        return None;
    }

    // Included angle = 4 * atan(bulge)
    let b = bulge;
    let b2 = b * b;

    // Radius: r = chord * (1 + b²) / (4 * |b|)
    let r = chord_len * (1.0 + b2) / (4.0 * b.abs());

    // Perpendicular distance from midpoint to center:
    // d = r * cos(theta/2), where cos(theta/2) = (1 - b²) / (1 + b²)
    let d = r * (1.0 - b2) / (1.0 + b2);

    // Midpoint
    let mx = (p0[0] + p1[0]) * 0.5;
    let my = (p0[1] + p1[1]) * 0.5;

    // Left-perpendicular direction (rotate chord 90° CCW)
    let perp_x = -chord_y / chord_len;
    let perp_y = chord_x / chord_len;

    // Positive bulge → center is to the left of chord direction (CCW arc)
    let sign = b.signum();
    let cx = mx + sign * d * perp_x;
    let cy = my + sign * d * perp_y;

    // Angles from center to p0 and p1
    let a0_rad = (p0[1] - cy).atan2(p0[0] - cx);
    let a1_rad = (p1[1] - cy).atan2(p1[0] - cx);

    // acadrust arcs are always CCW: positive bulge → CCW from p0 to p1
    // Negative bulge → CW from p0 to p1 = CCW from p1 to p0
    let (start_angle, end_angle) = if b > 0.0 {
        (norm_angle(a0_rad), norm_angle(a1_rad))
    } else {
        (norm_angle(a1_rad), norm_angle(a0_rad))
    };

    let mut common = common_src.clone();
    common.handle = Handle::NULL;

    let arc = ArcEnt {
        common,
        center: Vector3::new(cx, cy, elevation),
        radius: r,
        start_angle,
        end_angle,
        ..ArcEnt::new()
    };
    Some(EntityType::Arc(arc))
}

fn norm_angle(a: f64) -> f64 {
    ((a % TAU) + TAU) % TAU
}

fn explode_mline(ml: &MLine) -> Vec<EntityType> {
    let n = ml.vertices.len();
    if n < 2 {
        return vec![];
    }
    let closed = ml.flags.contains(acadrust::entities::MLineFlags::CLOSED);
    let scale = ml.scale_factor;
    let n_segs = if closed { n } else { n - 1 };
    let mut result = Vec::new();

    // Helper: build a Line from two Vector3 positions.
    let make_line = |common: &acadrust::entities::EntityCommon,
                     s: &acadrust::types::Vector3,
                     e: &acadrust::types::Vector3| -> EntityType {
        let mut c = common.clone();
        c.handle = Handle::NULL;
        EntityType::Line(LineEnt { common: c, start: s.clone(), end: e.clone(), ..LineEnt::new() })
    };

    // For each segment, emit the center-spine line and the two ±scale/2 offset lines.
    for i in 0..n_segs {
        let v0 = &ml.vertices[i];
        let v1 = &ml.vertices[(i + 1) % n];

        // Spine line
        result.push(make_line(&ml.common, &v0.position, &v1.position));

        if scale.abs() > 1e-9 {
            let half = scale * 0.5;
            for &sign in &[-1.0_f64, 1.0_f64] {
                let off = half * sign;
                // Use miter direction at each vertex to offset the endpoints.
                let s = Vector3::new(
                    v0.position.x + v0.miter.x * off,
                    v0.position.y + v0.miter.y * off,
                    v0.position.z + v0.miter.z * off,
                );
                let e = Vector3::new(
                    v1.position.x + v1.miter.x * off,
                    v1.position.y + v1.miter.y * off,
                    v1.position.z + v1.miter.z * off,
                );
                result.push(make_line(&ml.common, &s, &e));
            }
        }
    }

    result
}

// ── Dimension explode ──────────────────────────────────────────────────────

/// Convert a Dimension entity into Lines (geometry) + Text (label).
fn explode_dimension(dim: &Dimension) -> Vec<EntityType> {
    use acadrust::entities::Text;

    let base = dim.base();
    let common = base.common.clone();
    let mut result: Vec<EntityType> = Vec::new();

    // Helper: make a line segment
    let make_seg = |a: &Vector3, b: &Vector3, common: &EntityCommon| -> EntityType {
        let mut c = common.clone();
        c.handle = Handle::NULL;
        EntityType::Line(LineEnt { common: c, start: a.clone(), end: b.clone(), ..LineEnt::new() })
    };

    let v3 = |x: f64, y: f64, z: f64| Vector3::new(x, y, z);

    match dim {
        Dimension::Aligned(d) => {
            let fx = d.first_point.x;  let fy = d.first_point.y;
            let sx = d.second_point.x; let sy = d.second_point.y;
            let dx_s = sx - fx; let dy_s = sy - fy;
            let len = (dx_s*dx_s + dy_s*dy_s).sqrt().max(1e-12);
            let axis_angle = dy_s.atan2(dx_s);
            let perp_x = -(axis_angle.sin()); let perp_y = axis_angle.cos();
            let offset = (d.definition_point.x - fx) * perp_x + (d.definition_point.y - fy) * perp_y;
            let d1 = v3(fx + perp_x * offset, fy + perp_y * offset, d.first_point.z);
            let d2 = v3(sx + perp_x * offset, sy + perp_y * offset, d.second_point.z);
            result.push(make_seg(&d.first_point, &d1, &common));
            result.push(make_seg(&d.second_point, &d2, &common));
            result.push(make_seg(&d1, &d2, &common));
            let _ = len;
        }
        Dimension::Linear(d) => {
            let angle = d.rotation.to_radians();
            let perp_x = -(angle.sin()); let perp_y = angle.cos();
            let fx = d.first_point.x; let fy = d.first_point.y;
            let sx = d.second_point.x; let sy = d.second_point.y;
            let offset = (d.definition_point.x - fx) * perp_x + (d.definition_point.y - fy) * perp_y;
            let d1 = v3(fx + perp_x * offset, fy + perp_y * offset, d.first_point.z);
            let d2 = v3(sx + perp_x * offset, sy + perp_y * offset, d.second_point.z);
            result.push(make_seg(&d.first_point, &d1, &common));
            result.push(make_seg(&d.second_point, &d2, &common));
            result.push(make_seg(&d1, &d2, &common));
        }
        Dimension::Radius(d) => {
            result.push(make_seg(&d.angle_vertex, &d.definition_point, &common));
        }
        Dimension::Diameter(d) => {
            result.push(make_seg(&d.angle_vertex, &d.definition_point, &common));
        }
        Dimension::Angular2Ln(d) => {
            result.push(make_seg(&d.first_point, &d.angle_vertex, &common));
            result.push(make_seg(&d.second_point, &d.angle_vertex, &common));
        }
        Dimension::Angular3Pt(d) => {
            result.push(make_seg(&d.first_point, &d.angle_vertex, &common));
            result.push(make_seg(&d.second_point, &d.angle_vertex, &common));
        }
        Dimension::Ordinate(d) => {
            result.push(make_seg(&d.feature_location, &d.definition_point, &common));
            result.push(make_seg(&d.definition_point, &d.leader_endpoint, &common));
        }
    }

    // Text entity for the dimension label
    let text_val = if let Some(u) = &base.user_text {
        if !u.trim().is_empty() { u.clone() } else { format!("{:.4}", dim.measurement()) }
    } else if !base.text.trim().is_empty() {
        base.text.clone()
    } else {
        match dim {
            Dimension::Radius(_)   => format!("R{:.4}", dim.measurement()),
            Dimension::Diameter(_) => format!("Ø{:.4}", dim.measurement()),
            Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) =>
                format!("{:.2}°", dim.measurement()),
            _ => format!("{:.4}", dim.measurement()),
        }
    };

    let mut text = Text::with_value(text_val, base.text_middle_point.clone())
        .with_height(base.line_spacing_factor.abs().max(0.1))
        .with_rotation(base.text_rotation);
    text.common = common.clone();
    text.common.handle = Handle::NULL;
    result.push(EntityType::Text(text));

    result
}

// ── Command stub (kept for future interactive selection mode) ───────────────

pub struct ExplodeCommand;

impl ExplodeCommand {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl CadCommand for ExplodeCommand {
    fn name(&self) -> &'static str {
        "EXPLODE"
    }
    fn prompt(&self) -> String {
        "EXPLODE  Select objects to explode:".into()
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
