// TOLERANCE command — place a GD&T (geometric dimensioning & tolerancing) frame.
//
// The structured editor prepares the frame; this command places it.

use acadrust::entities::Tolerance;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/tolerance.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TOLERANCE",
        label: "Tolerance",
        icon: ICON,
        event: ModuleEvent::Command("TOLERANCE".to_string()),
    }
}

pub struct ToleranceCommand {
    text: String,
    plane: WorkingPlane,
}

impl ToleranceCommand {
    pub fn with_text(text: String) -> Self {
        Self {
            text,
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ToleranceCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TOLERANCE"
    }

    fn prompt(&self) -> String {
        t!("TOLERANCE  Specify insertion point:").into_owned()
    }

    fn wants_text_input(&self) -> bool {
        false
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let point = self.plane.to_local(pt);
        let ins = Vector3::new(point.x, point.y, point.z);
        let tol = Tolerance::with_text(ins, self.text.clone());
        CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Tolerance(tol)))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let normalized = self
            .text
            .replace("^J", "\n")
            .replace("\\P", "\n")
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        let rows: Vec<Vec<&str>> = normalized
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let mut cells: Vec<&str> = line.split("%%v").collect();
                while cells.last().is_some_and(|cell| cell.trim().is_empty()) {
                    cells.pop();
                }
                cells
            })
            .filter(|cells| !cells.is_empty())
            .collect();
        let row_height = 0.35;
        let cell_width = 0.5;
        let mut points = Vec::new();
        let mut segment = |a: DVec3, b: DVec3| {
            if !points.is_empty() {
                points.push(DVec3::splat(f64::NAN));
            }
            points.push(a);
            points.push(b);
        };
        for (row_index, cells) in rows.iter().enumerate() {
            let count = cells.len().max(1);
            let y0 = -row_height * (row_index as f64 + 0.5);
            let y1 = y0 + row_height;
            let x1 = cell_width * count as f64;
            let p = |x: f64, y: f64| pt + self.plane.x * x + self.plane.y * y;
            segment(p(0.0, y0), p(x1, y0));
            segment(p(x1, y0), p(x1, y1));
            segment(p(x1, y1), p(0.0, y1));
            segment(p(0.0, y1), p(0.0, y0));
            for index in 1..count {
                let x = cell_width * index as f64;
                segment(p(x, y0), p(x, y1));
            }
        }
        Some(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "tolerance_preview".into(),
            points: points.iter().map(|point| point.as_vec3().to_array()).collect(),
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        })
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TOLERANCE"] });  // ToleranceCommand
