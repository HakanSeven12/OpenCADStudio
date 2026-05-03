// LENGTHEN command — extend or trim a Line or Arc by a specified delta or total.
//
// Options (entered as text after the entity pick):
//   DE <value>   — extend by delta (positive extends, negative trims)
//   TO <value>   — set total length (Line) or arc length (Arc)
//   P <pct>      — change by percentage (100 = no change, 150 = +50%)
//
// The entity is modified at whichever end is closest to the pick point.

use acadrust::entities::{Arc as ArcEnt, Ellipse as EllipseEnt, Line as LineEnt, LwPolyline, Spline as SplineEnt};
use truck_modeling::base::{BoundedCurve, Cut};
use crate::modules::home::modify::spline_ops::{bspline_to_spline, spline_to_bspline};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::{CadCommand, CmdResult};

pub struct LengthenCommand {
    state: LenState,
}

enum LenState {
    PickEntity,
    PickOption { handle: Handle, pick_pt: Vec3 },
}

impl LengthenCommand {
    pub fn new() -> Self {
        Self { state: LenState::PickEntity }
    }
}

impl CadCommand for LengthenCommand {
    fn name(&self) -> &'static str { "LENGTHEN" }

    fn prompt(&self) -> String {
        match &self.state {
            LenState::PickEntity => "LENGTHEN  Select object:".into(),
            LenState::PickOption { .. } =>
                "LENGTHEN  Enter option [DE <delta> / TO <total> / P <pct>]:".into(),
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.state, LenState::PickEntity)
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: Vec3) -> CmdResult {
        if handle.is_null() { return CmdResult::NeedPoint; }
        self.state = LenState::PickOption { handle, pick_pt: pt };
        CmdResult::NeedPoint
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.state, LenState::PickOption { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let (handle, pick_pt) = match &self.state {
            LenState::PickOption { handle, pick_pt } => (*handle, *pick_pt),
            _ => return None,
        };

        let text = text.trim().to_uppercase();
        if let Some(rest) = text.strip_prefix("DE ").or_else(|| text.strip_prefix("DE")) {
            let delta: f64 = rest.trim().replace(',', ".").parse().ok()?;
            Some(CmdResult::LengthenEntity { handle, pick_pt, mode: LenMode::Delta(delta) })
        } else if let Some(rest) = text.strip_prefix("TO ").or_else(|| text.strip_prefix("TO")) {
            let total: f64 = rest.trim().replace(',', ".").parse().ok().filter(|&v: &f64| v > 0.0)?;
            Some(CmdResult::LengthenEntity { handle, pick_pt, mode: LenMode::Total(total) })
        } else if let Some(rest) = text.strip_prefix("P ").or_else(|| text.strip_prefix("P")) {
            let pct: f64 = rest.trim().replace(',', ".").parse().ok().filter(|&v: &f64| v > 0.0)?;
            Some(CmdResult::LengthenEntity { handle, pick_pt, mode: LenMode::Percent(pct) })
        } else {
            // Try plain number as delta
            let delta: f64 = text.replace(',', ".").parse().ok()?;
            Some(CmdResult::LengthenEntity { handle, pick_pt, mode: LenMode::Delta(delta) })
        }
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
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
        EntityType::Line(l)       => lengthen_line(l, pick_pt, mode),
        EntityType::Arc(a)        => lengthen_arc(a, pick_pt, mode),
        EntityType::Ellipse(e)    => lengthen_ellipse(e, pick_pt, mode),
        EntityType::Spline(s)     => lengthen_spline(s, pick_pt, mode),
        EntityType::LwPolyline(p) => lengthen_lwpoly(p, pick_pt, mode),
        _ => None,
    }
}

fn lengthen_line(line: &LineEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let s = Vec3::new(line.start.x as f32, line.start.z as f32, 0.0);
    let e = Vec3::new(line.end.x as f32,   line.end.z as f32,   0.0);
    let p = Vec3::new(pick_pt.x, pick_pt.z, 0.0);

    let current_len = (e - s).length() as f64;
    if current_len < 1e-10 { return None; }

    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 { return None; }

    let dir = (e - s) / current_len as f32;

    // Which end is closer to pick?
    let dist_to_start = (p - s).length();
    let dist_to_end   = (p - e).length();

    let mut result = line.clone();
    result.common.handle = Handle::NULL;

    if dist_to_end <= dist_to_start {
        // Extend/trim the end
        let new_end = s + dir * new_len as f32;
        result.end = xz_to_v3(new_end, line.end.z);
    } else {
        // Extend/trim the start (move start backward along dir)
        let new_start = e - dir * new_len as f32;
        result.start = xz_to_v3(new_start, line.start.z);
    }
    Some(EntityType::Line(result))
}

fn lengthen_arc(arc: &ArcEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let cx = arc.center.x as f32;
    let cy = arc.center.z as f32; // Y-up: DXF Y → world Z

    // Current arc span
    let span = arc_span_rad(arc.start_angle, arc.end_angle);
    let current_arc_len = arc.radius * span;

    let new_arc_len = apply_mode(current_arc_len, mode)?;
    if new_arc_len < 1e-10 { return None; }
    let new_span = new_arc_len / arc.radius;

    // Which end (start or end angle) is closer to pick?
    let start_rad = arc.start_angle;
    let end_rad   = arc.end_angle;

    let start_pt = Vec3::new(
        cx + arc.radius as f32 * start_rad.cos() as f32,
        pick_pt.y,
        cy + arc.radius as f32 * start_rad.sin() as f32,
    );
    let end_pt = Vec3::new(
        cx + arc.radius as f32 * end_rad.cos() as f32,
        pick_pt.y,
        cy + arc.radius as f32 * end_rad.sin() as f32,
    );
    let dist_start = (pick_pt - start_pt).length();
    let dist_end   = (pick_pt - end_pt).length();

    let delta_span = new_span - span;

    let mut result = arc.clone();
    result.common.handle = Handle::NULL;

    if dist_end <= dist_start {
        // Extend end angle
        result.end_angle = arc.start_angle + new_span;
    } else {
        // Extend start angle (move start backwards)
        result.start_angle = arc.end_angle - new_span;
    }
    let _ = delta_span;
    Some(EntityType::Arc(result))
}

fn lengthen_ellipse(ell: &EllipseEnt, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let a = (ell.major_axis.x.powi(2) + ell.major_axis.y.powi(2)).sqrt();
    if a < 1e-9 { return None; }
    let b = a * ell.minor_axis_ratio;
    let nx = ell.major_axis.x / a;
    let ny = ell.major_axis.y / a;

    let t0 = ell.start_parameter;
    let mut t1 = ell.end_parameter;
    if t1 <= t0 { t1 += std::f64::consts::TAU; }
    let span = t1 - t0;

    // Approximate arc length via 128-point Gaussian quadrature estimate.
    let arc_len_approx = |span: f64| -> f64 {
        let n = 128usize;
        let mut len = 0.0;
        for i in 0..n {
            let ti = t0 + span * (i as f64 / n as f64);
            let tip = t0 + span * ((i + 1) as f64 / n as f64);
            let xi = a * ti.cos() * nx - b * ti.sin() * ny + ell.center.x;
            let yi = a * ti.cos() * ny + b * ti.sin() * nx + ell.center.y;
            let xip = a * tip.cos() * nx - b * tip.sin() * ny + ell.center.x;
            let yip = a * tip.cos() * ny + b * tip.sin() * nx + ell.center.y;
            len += (xip - xi).hypot(yip - yi);
        }
        len
    };

    let current_len = arc_len_approx(span);
    if current_len < 1e-10 { return None; }

    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 { return None; }

    // Find the new span via bisection so that arc_len_approx(new_span) ≈ new_len.
    let max_span = std::f64::consts::TAU;
    let mut lo = 0.0f64;
    let mut hi = max_span;
    for _ in 0..40 {
        let mid = (lo + hi) * 0.5;
        if arc_len_approx(mid) < new_len { lo = mid; } else { hi = mid; }
    }
    let new_span = (lo + hi) * 0.5;

    // Determine which end is closer to pick_pt (use DXF XY plane).
    let p_x = pick_pt.x as f64;
    let p_y = pick_pt.z as f64; // Y-up: world Z → DXF Y
    let pt_start_x = ell.center.x + a * t0.cos() * nx - b * t0.sin() * ny;
    let pt_start_y = ell.center.y + a * t0.cos() * ny + b * t0.sin() * nx;
    let pt_end_x   = ell.center.x + a * t1.cos() * nx - b * t1.sin() * ny;
    let pt_end_y   = ell.center.y + a * t1.cos() * ny + b * t1.sin() * nx;
    let dist_start = (p_x - pt_start_x).hypot(p_y - pt_start_y);
    let dist_end   = (p_x - pt_end_x).hypot(p_y - pt_end_y);

    let mut result = ell.clone();
    result.common.handle = Handle::NULL;

    if dist_end <= dist_start {
        result.end_parameter = t0 + new_span;
    } else {
        result.start_parameter = t1 - new_span;
    }
    Some(EntityType::Ellipse(result))
}

fn apply_mode(current: f64, mode: &LenMode) -> Option<f64> {
    match mode {
        LenMode::Delta(d)   => Some(current + d),
        LenMode::Total(t)   => Some(*t),
        LenMode::Percent(p) => Some(current * p / 100.0),
    }
}

fn arc_span_rad(start: f64, end: f64) -> f64 {
    let span = (end - start).rem_euclid(std::f64::consts::TAU);
    if span < 1e-6 { std::f64::consts::TAU } else { span }
}

fn xz_to_v3(v: Vec3, z: f64) -> Vector3 {
    // v is (world_x, world_z, 0) → DXF (x, world_z, z)
    Vector3::new(v.x as f64, v.y as f64, z)
}

fn lengthen_lwpoly(poly: &LwPolyline, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    let n = poly.vertices.len();
    if n < 2 { return None; }

    // Determine which end is closer to the pick point (DXF XY: pick_pt.x, pick_pt.z).
    let px = pick_pt.x as f64;
    let py = pick_pt.z as f64; // Y-up: world Z = DXF Y

    let first = &poly.vertices[0];
    let last  = &poly.vertices[n - 1];
    let d_first = (first.location.x - px).hypot(first.location.y - py);
    let d_last  = (last.location.x  - px).hypot(last.location.y  - py);
    let at_end = d_last <= d_first;

    // Terminal segment direction and current length.
    let (sx, sy, ex, ey) = if at_end {
        (poly.vertices[n - 2].location.x, poly.vertices[n - 2].location.y,
         last.location.x, last.location.y)
    } else {
        (poly.vertices[1].location.x, poly.vertices[1].location.y,
         first.location.x, first.location.y)
    };

    let dx = ex - sx;
    let dy = ey - sy;
    let current_len = (dx * dx + dy * dy).sqrt();
    if current_len < 1e-10 { return None; }

    let new_len = apply_mode(current_len, mode)?;
    if new_len < 1e-10 { return None; }

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
    let bs = spline_to_bspline(spl)?;
    let (t0, t1) = bs.range_tuple();
    if (t1 - t0).abs() < 1e-12 { return None; }

    // Approximate arc length via 64-point numerical integration.
    use truck_modeling::base::ParametricCurve;
    let arc_len = {
        let n = 64usize;
        let mut len = 0.0f64;
        for i in 0..n {
            let ta = t0 + (t1 - t0) * (i as f64 / n as f64);
            let tb = t0 + (t1 - t0) * ((i + 1) as f64 / n as f64);
            let pa = bs.subs(ta);
            let pb = bs.subs(tb);
            len += (pb.x - pa.x).hypot(pb.y - pa.y);
        }
        len
    };
    if arc_len < 1e-10 { return None; }

    let new_len = apply_mode(arc_len, mode)?;
    if new_len < 1e-10 { return None; }

    // Determine which end (start or end) is closer to pick_pt.
    let p_start = bs.subs(t0);
    let p_end   = bs.subs(t1);
    let dist_start = (p_start.x - pick_pt.x as f64).hypot(p_start.y - pick_pt.z as f64);
    let dist_end   = (p_end.x   - pick_pt.x as f64).hypot(p_end.y   - pick_pt.z as f64);
    let extend_end = dist_end <= dist_start;

    // Find the parameter `t_new` such that the arc length from the fixed end to t_new = new_len.
    // Use bisection on cumulative arc length.
    let fixed_t = if extend_end { t0 } else { t1 };
    let delta_ratio = new_len / arc_len;

    // Find t_new via bisection: cumulative_len(fixed_t..t_new) = new_len.
    let cum_len = |t_end_param: f64| -> f64 {
        let (lo, hi) = if extend_end { (t0, t_end_param) } else { (t_end_param, t1) };
        if hi <= lo { return 0.0; }
        let n = 32usize;
        let mut len = 0.0f64;
        for i in 0..n {
            let ta = lo + (hi - lo) * (i as f64 / n as f64);
            let tb = lo + (hi - lo) * ((i + 1) as f64 / n as f64);
            let pa = bs.subs(ta);
            let pb = bs.subs(tb);
            len += (pb.x - pa.x).hypot(pb.y - pa.y);
        }
        len
    };

    let (mut lo_t, mut hi_t) = if extend_end {
        (t0, t0 + delta_ratio * (t1 - t0) * 2.0)
    } else {
        (t1 - delta_ratio * (t1 - t0) * 2.0, t1)
    };
    // Clamp to valid range with some buffer for extension.
    let buf = (t1 - t0) * 0.5;
    lo_t = lo_t.max(t0 - buf);
    hi_t = hi_t.min(t1 + buf);

    for _ in 0..40 {
        let mid = (lo_t + hi_t) * 0.5;
        if cum_len(mid) < new_len {
            if extend_end { hi_t = mid; } else { lo_t = mid; }
        } else {
            if extend_end { lo_t = mid; } else { hi_t = mid; }
        }
    }
    let t_new = (lo_t + hi_t) * 0.5;
    let _ = fixed_t;

    // Split the spline at t_new.
    let mut piece = bs.clone();
    if extend_end {
        // Keep [t0, t_new]: cut at t_new
        let _right = piece.cut(t_new.clamp(t0 + 1e-10, t1 * 2.0));
        Some(EntityType::Spline(bspline_to_spline(&piece, spl)))
    } else {
        // Keep [t_new, t1]: cut at t_new, return the right portion
        let right = piece.cut(t_new.clamp(t0 * 0.5, t1 - 1e-10));
        Some(EntityType::Spline(bspline_to_spline(&right, spl)))
    }
}
