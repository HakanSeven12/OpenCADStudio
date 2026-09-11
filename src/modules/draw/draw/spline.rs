// Spline tool — ribbon definition + interactive command.
//
// Command:  SPLINE (SPL)
//   Click to add fit points. Enter (≥2 pts) → commits EntityType::Spline.

use crate::t;
use acadrust::types::Vector3;
use acadrust::{EntityType, Spline};

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
    choosing_knots: bool,
    choosing_tangent: bool,
    knot_parameterization: i32,
    begin_tangent: Vector3,
    end_tangent: Vector3,
}

impl SplineCommand {
    pub fn new() -> Self {
        Self {
            pts: Vec::new(),
            control_vertices: false,
            choosing_method: false,
            choosing_knots: false,
            choosing_tangent: false,
            knot_parameterization: 0,
            begin_tangent: Vector3::ZERO,
            end_tangent: Vector3::ZERO,
        }
    }

    pub fn control_vertices() -> Self {
        Self {
            control_vertices: true,
            ..Self::new()
        }
    }

    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.pts.len() < 2 {
            return None;
        }
        let mut spline = make_spline(
            &self.pts,
            closed,
            self.control_vertices,
        );
        spline.knot_parameterization = self.knot_parameterization;
        spline.begin_tangent = self.begin_tangent;
        spline.end_tangent = self.end_tangent;
        Some(EntityType::Spline(spline))
    }
}

/// Store the chosen construction method directly in the persistent spline.
fn make_spline(pts: &[DVec3], closed: bool, control_vertices: bool) -> Spline {
    let mut points: Vec<Vector3> = pts.iter().map(|p| Vector3::new(p.x, p.y, p.z)).collect();
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
                if index <= degree {
                    0.0
                } else if index >= count {
                    1.0
                } else {
                    (index - degree) as f64 / spans as f64
                }
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
    // Closed fit curves use the kernel's periodic interpolation rather than
    // appending a closing straight segment to an open interpolant.
    spline.flags.periodic = closed && !control_vertices;
    spline
}

/// Preview the exact representation that `build()` commits.
#[cfg(test)]
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
        if self.control_vertices {
            "SPLINECV"
        } else {
            "SPLINE"
        }
    }

    fn prompt(&self) -> String {
        if self.choosing_method {
            "SPLINE  Choose creation method [Fit/Control vertices]:".into()
        } else if self.choosing_knots {
            "SPLINE  Enter knot parameterization [Chord/Square root/Uniform]:".into()
        } else if self.choosing_tangent {
            "SPLINE  Specify tangent direction:".into()
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
            return vec![
                CmdOption::new("Fit", "FIT"),
                CmdOption::new("Control vertices", "CV"),
            ];
        }
        if self.choosing_tangent { return vec![]; }
        if self.choosing_knots {
            return vec![CmdOption::new("Chord", "CH"), CmdOption::new("Square root", "S"), CmdOption::new("Uniform", "U")];
        }
        if self.pts.is_empty() {
            let mut options = vec![CmdOption::new(t!("Method").as_ref(), "M")];
            if !self.control_vertices { options.push(CmdOption::new("Knots", "K")); }
            return options;
        }
        let mut opts = vec![];
        if self.pts.len() >= 3 { opts.push(CmdOption::new(t!("Close").as_ref(), "C")); }
        if !self.control_vertices { opts.push(CmdOption::new("Tangency", "T")); }
        // Undo only makes sense once a control point exists.
        opts.push(CmdOption::new(t!("Undo").as_ref(), "U"));
        opts.push(CmdOption::enter(t!("Done").as_ref()));
        opts
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.choosing_method || self.choosing_knots || !pt.is_finite() {
            return CmdResult::NeedPoint;
        }
        if self.choosing_tangent {
            let Some(anchor) = self.pts.last() else { return CmdResult::NeedPoint; };
            let Some(dir) = (pt - *anchor).try_normalize() else { return CmdResult::NeedPoint; };
            let tangent = Vector3::new(dir.x, dir.y, dir.z);
            if self.pts.len() == 1 { self.begin_tangent = tangent; }
            else { self.end_tangent = tangent; }
            self.choosing_tangent = false;
            if self.pts.len() > 1 {
                return self.build(false).map_or(CmdResult::NeedPoint, CmdResult::CommitAndExit);
            }
            return CmdResult::NeedPoint;
        }
        if self.pts.last().is_some_and(|last| last.distance_squared(pt) < 1e-20) {
            return CmdResult::NeedPoint;
        }
        self.pts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.choosing_method || self.choosing_knots {
            self.choosing_method = false;
            self.choosing_knots = false;
            return CmdResult::NeedPoint;
        }
        if self.choosing_tangent { return CmdResult::NeedPoint; }
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

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.pts.is_empty() { return None; }
        self.pts.pop();
        self.end_tangent = Vector3::ZERO;
        self.choosing_tangent = false;
        if self.pts.is_empty() { self.begin_tangent = Vector3::ZERO; }
        Some(CmdResult::NeedPoint)
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.choosing_tangent { return None; }
        if self.choosing_knots {
            self.knot_parameterization = match text.trim().to_ascii_uppercase().as_str() {
                "CH" | "CHORD" => 0,
                "S" | "SQUARE" | "SQUAREROOT" => 1,
                "U" | "UNIFORM" => 2,
                _ => return None,
            };
            self.choosing_knots = false;
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_uppercase().as_str() {
            "K" | "KNOTS" if self.pts.is_empty() && !self.control_vertices => {
                self.choosing_knots = true;
                Some(CmdResult::NeedPoint)
            }
            "T" | "TANGENCY" if !self.pts.is_empty() && !self.control_vertices => {
                self.choosing_tangent = true;
                Some(CmdResult::NeedPoint)
            }
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
            "C" | "CLOSE" if self.pts.len() >= 3 => match self.build(true) {
                Some(e) => Some(CmdResult::CommitAndExit(e)),
                None => Some(CmdResult::NeedPoint),
            },
            "U" | "UNDO" => {
                self.on_undo_step().or(Some(CmdResult::NeedPoint))
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.pts.is_empty() || self.choosing_method || self.choosing_knots || self.choosing_tangent {
            return None;
        }
        // Preview the committed construction method.
        self.pts.push(pt);
        let entity = self.build(false);
        self.pts.pop();
        let Some(EntityType::Spline(spline)) = entity else { return None; };
        let points = crate::entities::curve::spline_curve(&spline)
            .map(|curve| crate::entities::curve::curve_points(&curve))
            .unwrap_or_else(|| crate::entities::spline::measurement_polyline(&spline));
        Some(WireModel::solid(
            "rubber_band".into(),
            points.into_iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(),
            WireModel::CYAN,
            false,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_vertices_make_a_clamped_finite_spline() {
        let points = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(3.0, 0.0, 0.0),
        ];
        let spline = make_spline(&points, false, true);

        assert_eq!(spline.degree, 3);
        assert_eq!(spline.control_points.len(), 4);
        assert_eq!(spline.knots, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(spline.weights, vec![1.0; 4]);
        assert!(sample_curve(&points, false, true)
            .iter()
            .flatten()
            .all(|value| value.is_finite()));
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["SPLINE", "SPLINECV"]
}); // SplineCommand
