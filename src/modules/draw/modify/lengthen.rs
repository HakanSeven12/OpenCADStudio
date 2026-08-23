// LENGTHEN command — extend or trim a Line or Arc by a specified delta or total.
//
// Options (entered as text after the entity pick):
//   DE <value>   — extend by delta (positive extends, negative trims)
//   TO <value>   — set total length (Line) or arc length (Arc)
//   P <pct>      — change by percentage (100 = no change, 150 = +50%)
//
// The entity is modified at whichever end is closest to the pick point.

use crate::modules::draw::modify::spline_ops::{spline_cut, spline_to_nurbs};
use acadrust::entities::{
    Arc as ArcEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Spline as SplineEnt,
};
use cadkernel::geom2d::{
    Curve, Ellipse as KernelEllipse, EllipseArc as KernelEllipseArc,
};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::{DVec3, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdResult};

const TAU: f64 = std::f64::consts::TAU;

pub struct LengthenCommand {
    state: LenState,
}

enum LenState {
    PickEntity,
    PickOption { handle: Handle, pick_pt: Vec3 },
}

impl LengthenCommand {
    pub fn new() -> Self {
        Self {
            state: LenState::PickEntity,
        }
    }
}

impl CadCommand for LengthenCommand {
    fn name(&self) -> &'static str {
        "LENGTHEN"
    }

    fn prompt(&self) -> String {
        match &self.state {
            LenState::PickEntity => t!("LENGTHEN  Select object:").into_owned(),
            LenState::PickOption { .. } => {
                t!("LENGTHEN  Enter option [DE <delta> / TO <total> / P <pct>]:").into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.state, LenState::PickEntity)
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult { let pt = pt.as_vec3();
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        self.state = LenState::PickOption {
            handle,
            pick_pt: pt,
        };
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.state, LenState::PickOption { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.state, LenState::PickOption { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let (handle, pick_pt) = match &self.state {
            LenState::PickOption { handle, pick_pt } => (*handle, *pick_pt),
            _ => return None,
        };
        let pick_pt = pick_pt.as_dvec3();

        let text = text.trim().to_uppercase();
        if let Some(rest) = text.strip_prefix("DE ").or_else(|| text.strip_prefix("DE")) {
            let delta: f64 = rest.trim().replace(',', ".").parse().ok()?;
            Some(CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode: LenMode::Delta(delta),
            })
        } else if let Some(rest) = text.strip_prefix("TO ").or_else(|| text.strip_prefix("TO")) {
            let total: f64 = rest
                .trim()
                .replace(',', ".")
                .parse()
                .ok()
                .filter(|&v: &f64| v > 0.0)?;
            Some(CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode: LenMode::Total(total),
            })
        } else if let Some(rest) = text.strip_prefix("P ").or_else(|| text.strip_prefix("P")) {
            let pct: f64 = rest
                .trim()
                .replace(',', ".")
                .parse()
                .ok()
                .filter(|&v: &f64| v > 0.0)?;
            Some(CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode: LenMode::Percent(pct),
            })
        } else {
            // Try plain number as delta
            let delta: f64 = text.replace(',', ".").parse().ok()?;
            Some(CmdResult::LengthenEntity {
                handle,
                pick_pt,
                mode: LenMode::Delta(delta),
            })
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Mode enum (also used in CmdResult) ────────────────────────────────────

#[derive(Clone)]
pub enum LenMode {
    Delta(f64),
    Total(f64),
    Percent(f64),
}

// ── Geometry ───────────────────────────────────────────────────────────────

/// Apply LENGTHEN to a Line, Arc, Ellipse, or Spline.
/// `pick_pt` determines which end to extend/trim (closest end is modified).
pub fn lengthen_entity(entity: &EntityType, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    match entity {
        EntityType::Line(l) => lengthen_line(l, pick_pt, mode),
        EntityType::Arc(a) => lengthen_arc(a, pick_pt, mode),
        EntityType::Ellipse(e) => lengthen_ellipse(e, pick_pt, mode),
        EntityType::Spline(s) => lengthen_spline(s, pick_pt, mode),
        EntityType::LwPolyline(p) => {
            let p = crate::entities::curve::lwpolyline_world_xy(p)?;
            lengthen_lwpoly(&p, pick_pt, mode)
        }
        _ => None,
    }
}

fn lengthen_line(line: &LineEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    // Keep the line's own coordinates on the f64 grid; only pick_pt (screen-only,
    // nearest-end selection) widens for the distance compare.
    let sx = line.start.x;
    let sy = line.start.y;
    let ex = line.end.x;
    let ey = line.end.y;

    let dx = ex - sx;
    let dy = ey - sy;
    let current_len = (dx * dx + dy * dy).sqrt();
    if current_len < 1e-10 {
        return None;
    }

    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 {
        return None;
    }

    let ux = dx / current_len;
    let uy = dy / current_len;

    // Which end is closer to pick?
    let px = pick_pt.x as f64;
    let py = pick_pt.y as f64;
    let dist_to_start = (px - sx).hypot(py - sy);
    let dist_to_end = (px - ex).hypot(py - ey);

    let mut result = line.clone();
    result.common.handle = Handle::NULL;

    if dist_to_end <= dist_to_start {
        // Extend/trim the end
        let new_x = sx + ux * new_len;
        let new_y = sy + uy * new_len;
        result.end = Vector3::new(new_x, new_y, line.end.z);
    } else {
        // Extend/trim the start (move start backward along dir)
        let new_x = ex - ux * new_len;
        let new_y = ey - uy * new_len;
        result.start = Vector3::new(new_x, new_y, line.start.z);
    }
    Some(EntityType::Line(result))
}

fn lengthen_arc(arc: &ArcEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    // Current arc span
    let span = arc_span_rad(arc.start_angle, arc.end_angle);
    let current_arc_len = arc.radius * span;

    let new_arc_len = apply_mode(current_arc_len, mode)?;
    if new_arc_len < 1e-10 || new_arc_len >= std::f64::consts::TAU * arc.radius - 1.0e-9 {
        return None;
    }
    let new_span = new_arc_len / arc.radius;

    // Which end (start or end angle) is closer to pick?
    let curve = crate::entities::curve::arc_curve(arc);
    let start_pt = Vec3::from_array(curve.point_at(0.0).map(|value| value as f32));
    let end_pt = Vec3::from_array(curve.point_at(1.0).map(|value| value as f32));
    let dist_start = (pick_pt - start_pt).length();
    let dist_end = (pick_pt - end_pt).length();

    let mut result = arc.clone();
    result.common.handle = Handle::NULL;

    if dist_end <= dist_start {
        // Extend end angle
        result.end_angle = arc.start_angle + new_span;
    } else {
        // Extend start angle (move start backwards)
        result.start_angle = arc.end_angle - new_span;
    }
    Some(EntityType::Arc(result))
}

fn lengthen_ellipse(ell: &EllipseEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let a = (ell.major_axis.x.powi(2) + ell.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 {
        return None;
    }
    let b = a * ell.minor_axis_ratio;
    let nx = ell.major_axis.x / a;
    let ny = ell.major_axis.y / a;

    let t0 = ell.start_parameter;
    let mut t1 = ell.end_parameter;
    if t1 <= t0 {
        t1 += TAU;
    }

    // Measured by the kernel rather than by a hundred and twenty-eight
    // chords: the chord sum reads short, so LENGTHEN's idea of "current" was
    // already below the true length before a delta was applied to it.
    let shape = KernelEllipse {
        centre: [ell.center.x, ell.center.y],
        major_radius: a,
        minor_radius: b,
        major_axis: [nx, ny],
    };
    let arc = |from: f64, to: f64| {
        Curve::Ellipse(KernelEllipseArc {
            ellipse: shape,
            start_parameter: from,
            end_parameter: to,
        })
    };
    let current_len = arc(t0, t1).length();
    if current_len < 1e-10 {
        return None;
    }
    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 {
        return None;
    }

    // Which end is closer to the pick, in the DXF XY plane.
    let point_at = |t: f64| {
        (
            ell.center.x + a * t.cos() * nx - b * t.sin() * ny,
            ell.center.y + a * t.cos() * ny + b * t.sin() * nx,
        )
    };
    let (p_x, p_y) = (pick_pt.x as f64, pick_pt.y as f64);
    let (sx, sy) = point_at(t0);
    let (ex, ey) = point_at(t1);
    let extend_end = (p_x - ex).hypot(p_y - ey) <= (p_x - sx).hypot(p_y - sy);

    let mut result = ell.clone();
    result.common.handle = Handle::NULL;
    if extend_end {
        // Walk `new_len` forward from the fixed start. A whole turn is the
        // most there is to walk, and the kernel clamps to it.
        let whole = arc(t0, t0 + TAU);
        result.end_parameter = t0 + whole.parameter_at_distance(new_len) * TAU;
    } else {
        // The same measured backwards from the fixed end: the last `new_len`
        // of a whole turn ending at t1.
        let whole = arc(t1 - TAU, t1);
        let from_start = whole.length() - new_len;
        result.start_parameter = t1 - TAU + whole.parameter_at_distance(from_start) * TAU;
    }
    Some(EntityType::Ellipse(result))
}


fn apply_mode(current: f64, mode: &LenMode) -> Option<f64> {
    match mode {
        LenMode::Delta(d) => Some(current + d),
        LenMode::Total(t) => Some(*t),
        LenMode::Percent(p) => Some(current * p / 100.0),
    }
}

fn arc_span_rad(start: f64, end: f64) -> f64 {
    let span = (end - start).rem_euclid(TAU);
    if span < 1e-6 {
        TAU
    } else {
        span
    }
}

fn lengthen_lwpoly(poly: &LwPolyline, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 2 {
        return None;
    }

    // Determine which end is closer to the pick point (DXF XY: pick_pt.x, pick_pt.z).
    let px = pick_pt.x as f64;
    let py = pick_pt.y as f64;

    let first = &poly.vertices[0];
    let last = &poly.vertices[n - 1];
    let d_first = (first.location.x - px).hypot(first.location.y - py);
    let d_last = (last.location.x - px).hypot(last.location.y - py);
    let at_end = d_last <= d_first;

    // Terminal segment direction and current length.
    let (sx, sy, ex, ey) = if at_end {
        (
            poly.vertices[n - 2].location.x,
            poly.vertices[n - 2].location.y,
            last.location.x,
            last.location.y,
        )
    } else {
        (
            poly.vertices[1].location.x,
            poly.vertices[1].location.y,
            first.location.x,
            first.location.y,
        )
    };

    let dx = ex - sx;
    let dy = ey - sy;
    let current_len = (dx * dx + dy * dy).sqrt();
    if current_len < 1e-10 {
        return None;
    }

    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 {
        return None;
    }

    let ux = dx / current_len;
    let uy = dy / current_len;
    let new_x = sx + ux * new_len;
    let new_y = sy + uy * new_len;

    let mut new_poly = poly.clone();
    new_poly.common.handle = Handle::NULL;
    if at_end {
        let v = new_poly.vertices.last_mut()?;
        v.location.x = new_x;
        v.location.y = new_y;
    } else {
        let v = new_poly.vertices.first_mut()?;
        v.location.x = new_x;
        v.location.y = new_y;
    }
    Some(EntityType::LwPolyline(new_poly))
}

fn lengthen_spline(spl: &SplineEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let nurbs = spline_to_nurbs(spl)?;
    let (t0, t1) = nurbs.domain();
    if (t1 - t0).abs() < 1e-12 {
        return None;
    }
    let curve = Curve::Nurbs(nurbs.clone());
    let arc_len = curve.length();
    if arc_len < 1e-10 {
        return None;
    }
    let new_len = apply_mode(arc_len, mode)?;
    if new_len < 1e-10 || new_len >= arc_len {
        // A spline is shortened by splitting it, so there is nothing to keep
        // if the new length is the whole of it or more. Extending would mean
        // continuing the curve past its own control polygon, which is a
        // different operation from cutting one.
        return None;
    }

    let p_start = nurbs.point_at_knot(t0);
    let p_end = nurbs.point_at_knot(t1);
    let (px, py) = (pick_pt.x as f64, pick_pt.y as f64);
    let extend_end = (p_end[0] - px).hypot(p_end[1] - py)
        <= (p_start[0] - px).hypot(p_start[1] - py);

    // Where to cut, by distance along the curve rather than by a bisection
    // over repeated chord sums. Keeping the head means cutting `new_len` from
    // the start; keeping the tail means cutting what is left over.
    let along = if extend_end {
        new_len
    } else {
        arc_len - new_len
    };
    let at = curve.parameter_at_distance(along);
    let cut = (t0 + at * (t1 - t0)).clamp(t0 + 1e-10, t1 - 1e-10);
    let (left, right) = spline_cut(spl, cut)?;
    Some(EntityType::Spline(if extend_end { left } else { right }))
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["LENGTHEN"] });  // LengthenCommand
