// Spline tool — ribbon definition + interactive command.
//
// Command:  SPLINE (SPL)
//   Click to add fit points. Enter (≥2 pts) → commits EntityType::Spline.

use acadrust::types::Vector3;
use acadrust::{EntityType, Spline};
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SPLINE",
        label: "Spline",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/spline.svg")),
        event: ModuleEvent::Command("SPLINE".to_string()),
    }
}

pub struct SplineCommand {
    pts: Vec<DVec3>,
    control_vertices: bool,
    choosing_method: bool,
}

impl SplineCommand {
    pub fn new() -> Self {
        Self { pts: Vec::new(), control_vertices: false, choosing_method: false }
    }

    pub fn control_vertices() -> Self {
        Self { control_vertices: true, ..Self::new() }
    }

    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.pts.len() < 2 {
            return None;
        }
        Some(EntityType::Spline(make_spline(&self.pts, closed, self.control_vertices)))
    }
}

/// Store the chosen construction method directly in the persistent spline.
fn make_spline(pts: &[DVec3], closed: bool, control_vertices: bool) -> Spline {
    let mut points: Vec<Vector3> = pts
            .iter()
            .map(|p| Vector3::new(p.x, p.y, p.z))
            .collect();
    let mut spline = if control_vertices {
        if closed && pts.first() != pts.last() {
            points.push(points[0]);
        }
        let count = points.len();
        let degree = 3.min(count.saturating_sub(1));
        let spans = count - degree;
        // Open uniform knot vector: degree + 1 equal knots at each endpoint.
        let knots = (0..count + degree + 1)
            .map(|index| {
                if index <= degree { 0.0 }
                else if index >= count { 1.0 }
                else { (index - degree) as f64 / spans as f64 }
            })
            .collect();
        Spline {
            degree: degree as i32,
            control_points: points,
            knots,
            weights: vec![1.0; count],
            ..Default::default()
        }
    } else {
        Spline {
            degree: 3,
            fit_points: points,
            ..Default::default()
        }
    };
    spline.flags.closed = closed;
    spline
}

/// Preview the exact representation that `build()` commits.
fn sample_curve(pts: &[DVec3], closed: bool, control_vertices: bool) -> Vec<[f32; 3]> {
    if pts.len() < 2 {
        return pts
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
    }
    let spline = make_spline(pts, closed, control_vertices);
    crate::entities::curve::spline_curve(&spline)
        .map(|curve| crate::entities::curve::curve_points(&curve))
        .unwrap_or_else(|| crate::entities::spline::measurement_polyline(&spline))
        .into_iter()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect()
}

impl CadCommand for SplineCommand {
    fn name(&self) -> &'static str {
        if self.control_vertices { "SPLINECV" } else { "SPLINE" }
    }

    fn prompt(&self) -> String {
        if self.choosing_method {
            "SPLINE  Choose creation method [Fit/Control vertices]:".into()
        } else if self.pts.is_empty() && self.control_vertices {
            t!("SPLINE  Specify first control point:").into_owned()
        } else if self.pts.is_empty() {
            t!("SPLINE  Specify first point:").into_owned()
        } else {
            let n = self.pts.len();
            t!("SPLINE  Specify next point  [%{n} pts]:", n = n).into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.choosing_method {
            return vec![CmdOption::new("Fit", "FIT"), CmdOption::new("Control vertices", "CV")];
        }
        if self.pts.is_empty() {
            return vec![CmdOption::new(t!("Method").as_ref(), "M")];
        }
        let mut opts = vec![CmdOption::new(t!("Close").as_ref(), "C")];
        // Undo only makes sense once a control point exists.
        opts.push(CmdOption::new(t!("Undo").as_ref(), "U"));
        opts.push(CmdOption::enter(t!("Done").as_ref()));
        opts
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.choosing_method || !pt.is_finite() {
            return CmdResult::NeedPoint;
        }
        self.pts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.choosing_method {
            self.choosing_method = false;
            return CmdResult::NeedPoint;
        }
        match self.build(false) {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.pts.is_empty() && !self.choosing_method
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "M" | "METHOD" if self.pts.is_empty() => {
                self.choosing_method = true;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FIT" if self.pts.is_empty() => {
                self.control_vertices = false;
                self.choosing_method = false;
                Some(CmdResult::NeedPoint)
            }
            "CV" | "CONTROL" | "CONTROLVERTICES" if self.pts.is_empty() => {
                self.control_vertices = true;
                self.choosing_method = false;
                Some(CmdResult::NeedPoint)
            }
            "C" | "CLOSE" => match self.build(true) {
                Some(e) => Some(CmdResult::CommitAndExit(e)),
                None => Some(CmdResult::NeedPoint),
            },
            "U" | "UNDO" => {
                self.pts.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.pts.is_empty() {
            return None;
        }
        // Preview the committed construction method.
        let mut ctrl = self.pts.clone();
        ctrl.push(pt);
        Some(WireModel::solid(
            "rubber_band".into(),
            sample_curve(&ctrl, false, self.control_vertices),
            WireModel::CYAN,
            false,
        ))
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["SPLINE", "SPLINECV"] });  // SplineCommand
