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
}

impl SplineCommand {
    pub fn new() -> Self {
        Self { pts: Vec::new() }
    }

    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.pts.len() < 2 {
            return None;
        }
        let fit_points = self
            .pts
            .iter()
            .map(|p| Vector3::new(p.x, p.y, p.z))
            .collect();
        let mut spline = Spline {
            degree: 3,
            fit_points,
            ..Default::default()
        };
        spline.flags.closed = closed;
        Some(EntityType::Spline(spline))
    }
}

/// Preview through the same fit-point representation that `build()` commits.
fn sample_curve(pts: &[DVec3], closed: bool) -> Vec<[f32; 3]> {
    if pts.len() < 2 {
        return pts
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();
    }
    let mut spline = Spline {
        degree: 3,
        fit_points: pts
            .iter()
            .map(|p| Vector3::new(p.x, p.y, p.z))
            .collect(),
        ..Default::default()
    };
    spline.flags.closed = closed;
    crate::entities::curve::spline_curve(&spline)
        .map(|curve| crate::entities::curve::curve_points(&curve))
        .unwrap_or_else(|| crate::entities::spline::measurement_polyline(&spline))
        .into_iter()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect()
}

impl CadCommand for SplineCommand {
    fn name(&self) -> &'static str {
        "SPLINE"
    }

    fn prompt(&self) -> String {
        if self.pts.is_empty() {
            t!("SPLINE  Specify first point:").into_owned()
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
        // Preview the committed fit-point representation.
        let mut ctrl = self.pts.clone();
        ctrl.push(pt);
        Some(WireModel::solid(
            "rubber_band".into(),
            sample_curve(&ctrl, false),
            WireModel::CYAN,
            false,
        ))
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["SPLINE"] });  // SplineCommand
