// 3DPOLY creates open or closed non-planar paths.

use acadrust::entities::Polyline3D;
use acadrust::types::Vector3;
use acadrust::EntityType;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "3DPOLY",
        label: "3D Polyline",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("3DPOLY".to_string()),
    }
}

// ── Command implementation ────────────────────────────────────────────────

pub struct Poly3dCommand {
    /// Every point picked so far, in world space. `points[0]` is the start
    /// (needed by Close); each retains its own Z.
    points: Vec<DVec3>,
}

impl Poly3dCommand {
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    /// Build the 3D polyline entity from the collected points, or `None` when
    /// there are too few points to form a segment.
    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.points.len() < 2 {
            return None;
        }
        let pts: Vec<Vector3> = self
            .points
            .iter()
            .map(|p| Vector3::new(p.x, p.y, p.z))
            .collect();
        let mut pl = Polyline3D::from_points(pts);
        pl.flags.closed = closed;
        Some(EntityType::Polyline3D(pl))
    }
}

impl CadCommand for Poly3dCommand {
    fn name(&self) -> &'static str {
        "3DPOLY"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => t!("3DPOLY  Specify start point of polyline:").into_owned(),
            1 | 2 => t!("3DPOLY  Specify next point  [Undo]:").into_owned(),
            _ => t!("3DPOLY  Specify next point  [Close/Undo]:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.build(false) {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        !self.points.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        !self.points.is_empty()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match text.trim().to_uppercase().as_str() {
            "C" | "CLOSE" if self.points.len() >= 3 => self
                .build(true)
                .map(CmdResult::CommitAndExit),
            "C" | "CLOSE" => Some(CmdResult::NeedPoint),
            "U" | "UNDO" => {
                self.points.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.points.is_empty() {
            return None;
        }
        // Preview the whole chain committed so far plus the pending segment to
        // the cursor, so the path is visible while it is built.
        let mut pts: Vec<[f64; 3]> = self.points.iter().map(|p| [p.x, p.y, p.z]).collect();
        pts.push([pt.x, pt.y, pt.z]);
        Some(WireModel::solid_f64(
            "rubber_band".to_string(),
            pts,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["3DPOLY"] });  // Poly3dCommand
