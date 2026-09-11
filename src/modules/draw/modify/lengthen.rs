// LENGTHEN command — extend or trim a Line or Arc by a specified delta or total.
//
// Choose an option, enter its value, then pick objects repeatedly:
//   DE <value>   — extend by delta (positive extends, negative trims)
//   TO <value>   — set total length (Line) or arc length (Arc)
//   P <pct>      — change by percentage (100 = no change, 150 = +50%)
//
// The entity is modified at whichever end is closest to the pick point.

use crate::modules::draw::modify::spline_ops::{spline_cut, spline_to_nurbs};
use acadrust::entities::{
    Ellipse as EllipseEnt, LwPolyline, Spline as SplineEnt,
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
    picked: Option<EntityType>,
    measurement: Option<f64>,
    edits: usize,
}

#[derive(Clone, Copy)]
enum ValueMode { Delta, Total, Percent, DeltaAngle, TotalAngle, Dynamic }

struct LengthenDefaults { mode: ValueMode, delta: f64, total: f64, percent: f64, delta_angle: f64, total_angle: f64 }
static LENGTHEN_DEFAULTS: std::sync::Mutex<LengthenDefaults> = std::sync::Mutex::new(
    LengthenDefaults { mode: ValueMode::Total, delta: 0.0, total: 1.0, percent: 100.0, delta_angle: 0.0, total_angle: 180.0 / std::f64::consts::PI }
);

enum LenState {
    ChooseMode,
    Value(ValueMode),
    Apply(LenMode),
    DynamicPick,
    DynamicPoint { handle: Handle, entity: EntityType, pick: DVec3 },
}

impl LengthenCommand {
    pub fn new() -> Self {
        Self { state: LenState::ChooseMode, picked: None, measurement: None, edits: 0 }
    }
}

impl CadCommand for LengthenCommand {
    fn name(&self) -> &'static str { "LENGTHEN" }

    fn prompt(&self) -> String {
        match &self.state {
            LenState::ChooseMode => {
                let prompt = t!("LENGTHEN  Select an object to measure or [Delta/Percent/Total/Dynamic]:");
                self.measurement.map_or_else(|| prompt.to_string(), |length| format!("Length: {length:.4}  {prompt}"))
            }
            LenState::Value(ValueMode::Delta) => format!("LENGTHEN  Enter delta length <{}>:", LENGTHEN_DEFAULTS.lock().unwrap().delta),
            LenState::Value(ValueMode::Total) => format!("LENGTHEN  Enter total length <{}>:", LENGTHEN_DEFAULTS.lock().unwrap().total),
            LenState::Value(ValueMode::Percent) => format!("LENGTHEN  Enter percentage length <{}>:", LENGTHEN_DEFAULTS.lock().unwrap().percent),
            LenState::Value(ValueMode::DeltaAngle) => format!("LENGTHEN  Enter delta angle <{}>:", LENGTHEN_DEFAULTS.lock().unwrap().delta_angle),
            LenState::Value(ValueMode::TotalAngle) => format!("LENGTHEN  Enter total angle <{}>:", LENGTHEN_DEFAULTS.lock().unwrap().total_angle),
            LenState::DynamicPoint { .. } => "LENGTHEN  Specify new end point:".into(),
            LenState::Apply(_) | LenState::DynamicPick | LenState::Value(ValueMode::Dynamic) => t!("LENGTHEN  Select an object to change or [Undo]:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match self.state {
            LenState::ChooseMode => vec![CmdOption::new("Delta", "DE"), CmdOption::new("Percent", "P"), CmdOption::new("Total", "TO"), CmdOption::new("Dynamic", "DY")],
            LenState::Value(ValueMode::Delta | ValueMode::Total) => vec![CmdOption::new("Angle", "A")],
            LenState::Apply(_) | LenState::DynamicPick => vec![CmdOption::new("Undo", "U")],
            _ => Vec::new(),
        }
    }

    fn needs_entity_pick(&self) -> bool { matches!(self.state, LenState::ChooseMode | LenState::Apply(_) | LenState::DynamicPick) }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) { self.picked = Some(entity); }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() { return CmdResult::NeedPoint; }
        let Some(entity) = self.picked.take() else { return CmdResult::NeedPoint; };
        match &self.state {
            LenState::ChooseMode => {
                self.measurement = crate::entities::curve::entity_curve(&entity)
                    .map(|curve| curve.curve.length()).filter(|length| length.is_finite());
                CmdResult::NeedPoint
            }
            LenState::Apply(mode) => match lengthen_entity_precise(&entity, pt, mode) {
                Some(replacement) => CmdResult::ReplaceManyContinue(vec![(handle, vec![replacement])]),
                None => CmdResult::NeedPoint,
            },
            LenState::DynamicPick if matches!(entity, EntityType::Line(_) | EntityType::Arc(_)) => {
                self.state = LenState::DynamicPoint { handle, entity, pick: pt };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_entity_replaced(&mut self, _old: Handle, _new_handles: &[Handle]) { self.edits += 1; }
    fn wants_text_input(&self) -> bool { true }
    fn dyn_commit_as_text(&self) -> bool { matches!(self.state, LenState::Value(_)) }
    fn dyn_auto_sign_angle(&self) -> bool { false }
    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.state, LenState::Value(ValueMode::DeltaAngle | ValueMode::TotalAngle)) { crate::command::DynField::Angle }
        else if matches!(self.state, LenState::Value(_)) { crate::command::DynField::Scalar }
        else { crate::command::DynField::Point }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if matches!(self.state, LenState::DynamicPoint { .. }) { return None; }
        let upper = text.trim().to_uppercase();
        if upper.is_empty() { return Some(self.on_enter()); }
        if matches!(self.state, LenState::Apply(_) | LenState::DynamicPick) {
            return Some(if matches!(upper.as_str(), "U" | "UNDO") && self.edits > 0 {
                self.edits -= 1;
                CmdResult::UndoDocument
            } else { CmdResult::NeedPoint });
        }
        if matches!(self.state, LenState::ChooseMode) {
            let mut parts = upper.split_whitespace();
            let mode = match parts.next().unwrap_or("") {
                "D" | "DE" | "DELTA" => ValueMode::Delta,
                "T" | "TO" | "TOTAL" => ValueMode::Total,
                "P" | "PERCENT" => ValueMode::Percent,
                "DY" | "DYNAMIC" => ValueMode::Dynamic,
                _ => return Some(CmdResult::NeedPoint),
            };
            LENGTHEN_DEFAULTS.lock().unwrap().mode = mode;
            self.state = if matches!(mode, ValueMode::Dynamic) { LenState::DynamicPick } else { LenState::Value(mode) };
            if let Some(value) = parts.next() { return self.on_text_input(value); }
            return Some(CmdResult::NeedPoint);
        }
        let LenState::Value(mode) = self.state else { return Some(CmdResult::NeedPoint); };
        if matches!(upper.as_str(), "A" | "ANGLE") {
            self.state = match mode {
                ValueMode::Delta => LenState::Value(ValueMode::DeltaAngle),
                ValueMode::Total => LenState::Value(ValueMode::TotalAngle),
                _ => return Some(CmdResult::NeedPoint),
            };
            return Some(CmdResult::NeedPoint);
        }
        let Some(value) = upper.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()) else {
            return Some(CmdResult::NeedPoint);
        };
        if !matches!(mode, ValueMode::Delta | ValueMode::DeltaAngle) && value <= 0.0 { return Some(CmdResult::NeedPoint); }
        if matches!(mode, ValueMode::TotalAngle) && value >= 360.0 { return Some(CmdResult::NeedPoint); }
        {
            let mut defaults = LENGTHEN_DEFAULTS.lock().unwrap();
            match mode { ValueMode::Delta => defaults.delta = value, ValueMode::Total => defaults.total = value, ValueMode::Percent => defaults.percent = value,
                ValueMode::DeltaAngle => defaults.delta_angle = value, ValueMode::TotalAngle => defaults.total_angle = value, ValueMode::Dynamic => {} }
        }
        self.state = LenState::Apply(match mode {
            ValueMode::Delta => LenMode::Delta(value),
            ValueMode::Total => LenMode::Total(value),
            ValueMode::Percent => LenMode::Percent(value),
            ValueMode::DeltaAngle => LenMode::DeltaAngle(value.to_radians()),
            ValueMode::TotalAngle => LenMode::TotalAngle(value.to_radians()),
            ValueMode::Dynamic => return Some(CmdResult::NeedPoint),
        });
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let LenState::DynamicPoint { handle, entity, pick } = &self.state {
            if let Some(replacement) = lengthen_entity_precise(entity, *pick, &LenMode::Dynamic(pt)) {
                let handle = *handle;
                self.state = LenState::DynamicPick;
                return CmdResult::ReplaceManyContinue(vec![(handle, vec![replacement])]);
            }
        }
        CmdResult::NeedPoint
    }
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<crate::scene::model::wire_model::WireModel> {
        let LenState::DynamicPoint { entity, pick, .. } = &self.state else { return None; };
        let replacement = lengthen_entity_precise(entity, *pick, &LenMode::Dynamic(pt))?;
        let curve = crate::entities::curve::entity_curve(&replacement)?;
        let points = crate::entities::curve::curve_points(&curve).into_iter()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
        Some(crate::scene::model::wire_model::WireModel::solid(
            "lengthen_dynamic_preview".into(), points,
            crate::scene::model::wire_model::WireModel::CYAN, false,
        ))
    }
    fn on_enter(&mut self) -> CmdResult {
        match self.state {
            LenState::ChooseMode => {
                let mode = LENGTHEN_DEFAULTS.lock().unwrap().mode;
                self.state = if matches!(mode, ValueMode::Dynamic) { LenState::DynamicPick } else { LenState::Value(mode) };
                CmdResult::NeedPoint
            }
            LenState::Value(mode) => {
                let value = {
                    let defaults = LENGTHEN_DEFAULTS.lock().unwrap();
                    match mode { ValueMode::Delta => defaults.delta, ValueMode::Total => defaults.total, ValueMode::Percent => defaults.percent,
                        ValueMode::DeltaAngle => defaults.delta_angle, ValueMode::TotalAngle => defaults.total_angle, ValueMode::Dynamic => 0.0 }
                };
                self.on_text_input(&value.to_string()).unwrap_or(CmdResult::NeedPoint)
            }
            _ => CmdResult::Cancel,
        }
    }
}
// ── Mode enum (also used in CmdResult) ────────────────────────────────────

#[derive(Clone)]
pub enum LenMode {
    Delta(f64),
    Total(f64),
    Percent(f64),
    DeltaAngle(f64),
    TotalAngle(f64),
    Dynamic(DVec3),
}

// ── Geometry ───────────────────────────────────────────────────────────────

/// Apply LENGTHEN to a Line, Arc, Ellipse, or Spline.
/// `pick_pt` determines which end to extend/trim (closest end is modified).
pub fn lengthen_entity(entity: &EntityType, pick_pt: Vec3, mode: &LenMode) -> Option<EntityType> {
    lengthen_entity_precise(entity, pick_pt.as_dvec3(), mode)
}

fn lengthen_entity_precise(entity: &EntityType, pick: DVec3, mode: &LenMode) -> Option<EntityType> {
    use cadkernel::space::lengthen::{LengthChange, lengthen_line, lengthen_arc};
    let change = match mode {
        LenMode::Delta(value) => LengthChange::Delta(*value),
        LenMode::Total(value) => LengthChange::Total(*value),
        LenMode::Percent(value) => LengthChange::Percent(*value),
        LenMode::DeltaAngle(value) => LengthChange::DeltaAngle(*value),
        LenMode::TotalAngle(value) => LengthChange::TotalAngle(*value),
        LenMode::Dynamic(point) => LengthChange::Dynamic(point.to_array()),
    };
    match entity {
        EntityType::Line(line) => {
            let [start, end] = lengthen_line(
                [line.start.x, line.start.y, line.start.z],
                [line.end.x, line.end.y, line.end.z], pick.to_array(), change)?;
            let mut result = line.clone();
            result.common.handle = Handle::NULL;
            result.start = Vector3::new(start[0], start[1], start[2]);
            result.end = Vector3::new(end[0], end[1], end[2]);
            Some(EntityType::Line(result))
        }
        EntityType::Arc(arc) => {
            let (start, end) = lengthen_arc(&crate::entities::curve::arc_curve(arc), pick.to_array(), change)?;
            let mut result = arc.clone();
            result.common.handle = Handle::NULL;
            result.start_angle = start;
            result.end_angle = end;
            Some(EntityType::Arc(result))
        }
        EntityType::Ellipse(e) => lengthen_ellipse(e, pick.as_vec3(), mode),
        EntityType::Spline(s) => lengthen_spline(s, pick.as_vec3(), mode),
        EntityType::LwPolyline(p) => {
            let p = crate::entities::curve::lwpolyline_world_xy(p)?;
            lengthen_lwpoly(&p, pick.as_vec3(), mode)
        }
        _ => None,
    }
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
        _ => None,
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
