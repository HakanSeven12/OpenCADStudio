// Spline tool — ribbon definition + interactive command.
//
// Command:  SPLINE (SPL)
//   Click to add control points.  Enter (≥2 pts) → commits EntityType::Spline.

use acadrust::types::Vector3;
use acadrust::{EntityType, Spline};
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;
use acadrust::kernel::space::NurbsCurve3;

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
}

impl SplineCommand {
    pub fn new() -> Self {
        Self { pts: Vec::new() }
    }

    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.pts.len() < 2 {
            return None;
        }
        let control_points = self
            .pts
            .iter()
            .map(|p| Vector3::new(p.x, p.y, p.z))
            .collect();
        let n = self.pts.len();
        // Uniform open knot vector for degree-3 clamped B-spline
        let degree = 3_i32.min((n - 1) as i32);
        let knots = uniform_knots(n, degree as usize);
        let mut spline = Spline {
            degree,
            control_points,
            knots,
            ..Default::default()
        };
        // Closed splines render with a segment bridging the last point back to
        // the first (tessellation honours `flags.closed`).
        spline.flags.closed = closed;
        Some(EntityType::Spline(spline))
    }
}

fn uniform_knots(n: usize, d: usize) -> Vec<f64> {
    let m = n + d + 1;
    (0..m)
        .map(|i| {
            if i <= d {
                0.0
            } else if i >= m - d - 1 {
                1.0
            } else {
                (i - d) as f64 / (n - d) as f64
            }
        })
        .collect()
}

/// Sample the degree-3 clamped B-spline through `pts` into a dense polyline,
/// built with the same control points + knot vector `build()` commits so the
/// preview traces the exact final curve — not the straight control polygon.
/// Fewer than 2 points has no curve; the points are returned as-is.
fn sample_curve(pts: &[DVec3]) -> Vec<[f32; 3]> {
    let n = pts.len();
    if n < 2 {
        return pts
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
    }
    let degree = 3_usize.min(n - 1);
    let ctrl: Vec<[f64; 3]> = pts.iter().map(|p| [p.x, p.y, p.z]).collect();
    let Some(curve) = NurbsCurve3::new(degree, ctrl, uniform_knots(n, degree), None) else {
        return pts
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
    };
    // Resolution scales with span count so long splines stay smooth.
    let steps = 24 * (n - 1);
    (0..=steps)
        .map(|i| {
            let p = curve.point_at(i as f64 / steps as f64);
            [p[0] as f32, p[1] as f32, p[2] as f32]
        })
        .collect()
}

impl CadCommand for SplineCommand {
    fn name(&self) -> &'static str {
        "SPLINE"
    }

    fn prompt(&self) -> String {
        if self.pts.is_empty() {
            t!("SPLINE  Specify first control point:").into_owned()
        } else {
            let n = self.pts.len();
            t!("SPLINE  Specify next point  [%{n} pts]:", n = n).into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.pts.is_empty() {
            return Vec::new();
        }
        let mut opts = vec![CmdOption::new(t!("Close").as_ref(), "C")];
        // Undo only makes sense once a control point exists.
        opts.push(CmdOption::new(t!("Undo").as_ref(), "U"));
        opts.push(CmdOption::enter(t!("Done").as_ref()));
        opts
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.pts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // A spline gathers many points into ONE entity, so finishing must end
        // the command (CommitAndExit) — CommitEntity keeps the command armed
        // with the accumulated points and traps the cursor in a redraw loop.
        match self.build(false) {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.pts.is_empty()
    }

    fn on_escape(&mut self) -> CmdResult {
        match self.build(false) {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn wants_text_input(&self) -> bool {
        // Accept Close / Undo once a control point exists.
        !self.pts.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.pts.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
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
        // Preview the actual B-spline curve, treating the cursor as the next
        // control point, sampled exactly like the committed entity — not the
        // straight control polygon.
        let mut ctrl = self.pts.clone();
        ctrl.push(pt);
        Some(WireModel::solid(
            "rubber_band".into(),
            sample_curve(&ctrl),
            WireModel::CYAN,
            false,
        ))
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["SPLINE"] });  // SplineCommand
