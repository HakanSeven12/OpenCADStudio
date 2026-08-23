// 2D solid tool — interactive command.
//
// Command: SOLID (reachable as SO / SOLID2D) — pick three or four corner points
// and commit a filled triangle or quadrilateral. Four-point input is preserved
// in the entity's documented Z order; after a quadrilateral, its opposite edge
// starts the next connected shape. Enter after the third point commits a
// triangle.

use acadrust::entities::Solid;
use acadrust::types::Vector3;
use acadrust::EntityType;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SOLID2D",
        label: "2D Solid",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("SOLID2D".to_string()),
    }
}

// ── Command implementation ────────────────────────────────────────────────

pub struct Solid2dCommand {
    /// Corner points picked so far (3 → triangle, 4 → quadrilateral).
    points: Vec<DVec3>,
    plane: WorkingPlane,
}

impl Solid2dCommand {
    pub fn new() -> Self {
        Self {
            points: Vec::new(),
            plane: WorkingPlane::default(),
        }
    }

    fn v3(p: DVec3) -> Vector3 {
        Vector3::new(p.x, p.y, p.z)
    }

    fn solid_from_four_points(points: &[DVec3]) -> Solid {
        Solid::new(
            Self::v3(points[0]),
            Self::v3(points[1]),
            Self::v3(points[2]),
            Self::v3(points[3]),
        )
    }
}

impl CadCommand for Solid2dCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "SOLID"
    }

    fn prompt(&self) -> String {
        match self.points.len() {
            0 => t!("SOLID  Specify first point:").into_owned(),
            1 => t!("SOLID  Specify second point:").into_owned(),
            2 => t!("SOLID  Specify third point:").into_owned(),
            _ => t!("SOLID  Specify fourth point:").into_owned(),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        // After three corners Enter commits a triangle; offer a Done button.
        if self.points.len() == 3 {
            vec![CmdOption::enter(t!("Done").as_ref())]
        } else {
            Vec::new()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.points.push(pt);
        if self.points.len() == 4 {
            let local: Vec<DVec3> = self
                .points
                .iter()
                .map(|point| self.plane.to_local(*point))
                .collect();
            let solid = Self::solid_from_four_points(&local);
            let next_edge = [self.points[2], self.points[3]];
            self.points.clear();
            self.points.extend(next_edge);
            CmdResult::CommitEntity(self.plane.place_entity(EntityType::Solid(solid)))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.points.len() == 3 {
            let p: Vec<DVec3> = self
                .points
                .iter()
                .map(|point| self.plane.to_local(*point))
                .collect();
            let solid = Solid::triangle(Self::v3(p[0]), Self::v3(p[1]), Self::v3(p[2]));
            CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Solid(solid)))
        } else {
            CmdResult::Cancel
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.points.is_empty() {
            return None;
        }
        // Outline the exact Z-ordered boundary that would be committed. Do not
        // reorder a crossing shape: point order is intentional geometry.
        let mut preview = self.points.clone();
        preview.push(pt);
        let display_order: Vec<usize> = if preview.len() == 4 {
            vec![0, 1, 3, 2]
        } else {
            (0..preview.len()).collect()
        };
        let mut pts: Vec<[f64; 3]> = display_order
            .iter()
            .map(|index| {
                let point = preview[*index];
                [point.x, point.y, point.z]
            })
            .collect();
        pts.push([preview[0].x, preview[0].y, preview[0].z]);
        Some(WireModel::solid_f64(
            "rubber_band".to_string(),
            pts,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["SO", "SOLID", "SOLID2D"]
}); // Solid2dCommand
